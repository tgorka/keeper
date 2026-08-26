//! Epic 56's normal state, read by the three checks that used to call it damage
//! (Story 56.6, FR-339, NFR-41, AD-129).
//!
//! A folder that keeps pointers holds paths whose content is deliberately not
//! here. Every claim below is about the difference between that and real loss,
//! and none of them can be settled by a unit test, because each one turns on a
//! fact only a real repository has:
//!
//! 1. **The excuse is earned by every fact that is free, never by one.** The
//!    *index* says the bytes on disk are the pointer this repository committed;
//!    the compiled `VirtualPolicy` says that path is authorized to stay away;
//!    the object is genuinely absent rather than truncated; and where the
//!    remote is a directory this machine can see, that store holds the object.
//!    Any one alone is a guess, so each is removed in turn and the row comes
//!    back.
//!
//!    The policy in force is the `.keepervirtual` standing in the **worktree**
//!    — `VirtualPolicy::compile` never reads `HEAD`, and a profile-tier or
//!    folder-TOML `virtualPatterns` list would replace the file's wholesale.
//!    The fixture commits it anyway, because a real checkout has it committed
//!    and the committed pointer beside it is what the index arm needs.
//! 2. **The absence of a complaint is asserted over MANY paths.** One quiet path
//!    proves nothing — the failure this story must not create is a check that
//!    went quiet, and a fixture of one cannot tell the two apart. NFR-41's own
//!    fixture is a folder of thousands, so this one is a thousand.
//! 3. **The remote half still condemns.** `verify --remote` over a real batch
//!    server that answers a per-object 404 must still report the hole. A fix
//!    that silenced every check would pass everything above and fail this.
//! 4. **A repair pass writes nothing into the store.** Asserted by enumerating
//!    the store's objects on both sides of the call, which is the assertion that
//!    the write did not happen rather than that the answer was right.
//!
//! **Loopback only, and no resolver anywhere.** The offline half needs no server
//! at all and its remote is a *filesystem* remote whose LFS store this fixture
//! seeds per object. The two `--remote` tests bind `127.0.0.1:0` and answer the
//! batch themselves, so no name is ever resolved.
//!
//! **No `filter=lfs` rule is registered**, the same decision
//! `tests/dehydrate_entry.rs` documents: the pointer text is committed as an
//! ordinary small blob, which is exactly what a checkout of a virtual path
//! leaves in the index.

use std::collections::BTreeSet;
use std::io::{Read as _, Write as _};
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use keeper_sync::copy::{copy_verified, ContentSource, CopyOptions, CopyOutcome, CopyReport};
use keeper_sync::engine::Engine;
use keeper_sync::error::SyncError;
use keeper_sync::git;
use keeper_sync::lfs::hydrate::ContentRefusal;
use keeper_sync::lfs::pointer::Pointer;
use keeper_sync::lfs::store::LfsStore;
use keeper_sync::lfs::virtual_policy::VIRTUAL_PATTERN_FILE;
use keeper_sync::platform::{SyncPlatform, TestPlatform};
use keeper_sync::profile::{LfsMode, SyncProfile};
use keeper_sync::progress::SyncPhase;
use keeper_sync::provenance::{Provenance, SyncSource};

const PROFILE_ID: &str = "01JVIRTUALFAULT";

/// The zone the fixture's committed policy authorizes to stay away.
const ZONE: &str = "40-media";

/// A committed pointer the policy answers `Materialize` for, standing beside
/// the authorized ones. Its object is just as absent; the difference is that
/// nothing authorized it, so it is real loss.
const UNAUTHORIZED: &str = "30-docs/report.bin";

/// Big enough that a pointer standing in for it is unmistakable next to the
/// ~130 bytes of text on disk, small enough that a thousand of them cost
/// nothing.
const PAYLOAD_BYTES: usize = 4096;

// ---------------------------------------------------------------------------
// Fixture
// ---------------------------------------------------------------------------

/// `false` when this machine has no usable `git`, in which case there is no
/// index to read and nothing here is falsifiable — the same skip
/// `tests/dehydrate_entry.rs` makes.
fn init_repo(root: &Path) -> bool {
    std::process::Command::new("git")
        .args(["init", "-q", "-b", "main"])
        .current_dir(root)
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

fn profile(root: &Path, remote: &Path) -> SyncProfile {
    let mut p = SyncProfile::new(
        PROFILE_ID,
        "media",
        root,
        remote.to_string_lossy().into_owned(),
    );
    p.lfs_threshold_bytes = 1024;
    // The only mode in which a path is *meant* to hold nothing but its pointer.
    p.lfs_mode = LfsMode::PointerOnly;
    p
}

fn commit(repo: &gix::Repository, remote: &Path, added: Vec<PathBuf>) {
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
        &git::commit::StagedChange {
            added,
            ..Default::default()
        },
        &prov,
        &profile(root, remote),
        &sig,
        &std::collections::BTreeMap::new(),
        None,
    )
    .expect("commit");
}

/// Deterministic content for path `n`, distinct per path so no two share an oid.
fn payload(n: usize) -> Vec<u8> {
    (0..PAYLOAD_BYTES)
        .map(|i| (i.wrapping_mul(31).wrapping_add(n)) as u8)
        .collect()
}

/// The pointer for `content`, and the object itself deposited in `store`.
///
/// Test-only. Production mints a pointer through `lfs::stage::clean`, which is
/// the very call this story gates, so a fixture that used it would be asserting
/// against the thing under test.
fn seed_object(store: &LfsStore, content: &[u8]) -> Pointer {
    store.ensure_layout().expect("layout");
    let (oid, size) = store
        .insert_streaming(std::io::Cursor::new(content.to_vec()))
        .expect("seed the object store");
    Pointer::new(oid, size)
}

/// What the fixture put where.
struct Seeded {
    /// The authorized virtual paths, repository-relative and `/`-joined.
    virtual_paths: Vec<String>,
    /// Each path's committed pointer.
    pointers: Vec<Pointer>,
}

/// `count` committed pointers under `40-media/`, all matched by a committed
/// `.keepervirtual`, with every object seeded into the **remote's** store and
/// none into this machine's.
///
/// That is a checkout of a folder that keeps pointers, exactly: the index
/// carries the pointer, the worktree holds the pointer, the object lives
/// somewhere else. `None` on a machine with no usable `git`.
fn seed(root: &Path, remote: &Path, count: usize) -> Option<Seeded> {
    if !init_repo(root) {
        return None;
    }
    let remote_store = LfsStore::in_git_dir(remote);
    std::fs::create_dir_all(root.join(ZONE)).expect("create the zone");

    let mut virtual_paths = Vec::with_capacity(count);
    let mut pointers = Vec::with_capacity(count);
    let mut added = Vec::with_capacity(count + 1);
    for n in 0..count {
        let rela = format!("{ZONE}/clip-{n:05}.mp4");
        let pointer = seed_object(&remote_store, &payload(n));
        std::fs::write(root.join(&rela), pointer.render()).expect("check out the pointer");
        added.push(PathBuf::from(&rela));
        virtual_paths.push(rela);
        pointers.push(pointer);
    }

    // Written into the WORKTREE, which is the copy `VirtualPolicy::compile`
    // reads — it never looks at `HEAD`. Committed as well only because a real
    // checkout has it committed; nothing here depends on that.
    std::fs::write(root.join(VIRTUAL_PATTERN_FILE), format!("{ZONE}/**\n"))
        .expect("write the policy");
    added.push(PathBuf::from(VIRTUAL_PATTERN_FILE));

    let repo = git::repo::open(root, false).expect("open the fixture repository");
    commit(&repo, remote, added);
    drop(repo);

    Some(Seeded {
        virtual_paths,
        pointers,
    })
}

/// Commit [`UNAUTHORIZED`]: a pointer whose object is just as absent, outside
/// the zone the policy authorizes.
fn add_unauthorized(root: &Path, remote: &Path) -> Pointer {
    let pointer = seed_object(&LfsStore::in_git_dir(remote), &payload(usize::MAX));
    std::fs::create_dir_all(root.join(UNAUTHORIZED).parent().expect("parent"))
        .expect("create the docs zone");
    std::fs::write(root.join(UNAUTHORIZED), pointer.render()).expect("check out the pointer");
    let repo = git::repo::open(root, false).expect("reopen");
    commit(&repo, remote, vec![PathBuf::from(UNAUTHORIZED)]);
    pointer
}

fn engine_for(root: &Path, remote: &Path, data: &Path) -> Engine {
    engine_with(profile(root, remote), data)
}

/// The engine, or a panic.
///
/// **Never a skip.** `Engine::open(...).ok()?` turned every test in this file
/// into a vacuous pass the moment the constructor failed for any reason at all
/// — a bad migration, a platform regression, a data directory that could not be
/// written — and eleven silent passes is the worst possible answer to that.
/// `git` is the one genuine prerequisite here, and it is probed exactly once,
/// by [`init_repo`], for the reason its own doc gives.
fn engine_with(profile: SyncProfile, data: &Path) -> Engine {
    let platform = Arc::new(TestPlatform::new(data));
    let engine = Engine::open(platform as Arc<dyn SyncPlatform>).expect("open the engine");
    engine
        .upsert_profile(&profile)
        .expect("register the profile");
    engine
}

/// Every object file in a store, by name — the set a repair pass must not add
/// to.
fn store_objects(git_dir: &Path) -> BTreeSet<String> {
    let mut found = BTreeSet::new();
    let mut stack = vec![git_dir.join("lfs/objects")];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else {
                found.insert(
                    path.file_name()
                        .expect("name")
                        .to_string_lossy()
                        .into_owned(),
                );
            }
        }
    }
    found
}

fn off() -> AtomicBool {
    AtomicBool::new(false)
}

fn failure(report: &CopyReport, path: &str) -> String {
    let entry = report
        .entries
        .iter()
        .find(|entry| entry.path == path)
        .unwrap_or_else(|| panic!("no entry for {path}: {:?}", report.entries));
    match &entry.outcome {
        CopyOutcome::Failed { reason } => reason.clone(),
        other => panic!("expected a refusal for {path}, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// A real LFS batch server, so the remote half is proved rather than mocked
// ---------------------------------------------------------------------------

/// Answer `download` batches until the listener closes: every object asked
/// about is served, except `lost`, which comes back as a per-object 404.
///
/// A loop rather than one shot, because `audit_remote_objects` batches a few
/// hundred objects per request and `republish_missing_objects` audits again
/// before it repairs. A thread rather than a task, following
/// `tests/dehydrate_entry.rs`: nothing here may depend on a runtime yielding.
fn serve_batches(listener: std::net::TcpListener, lost: String) {
    std::thread::spawn(move || loop {
        let Ok((mut stream, _)) = listener.accept() else {
            return;
        };
        let mut head = Vec::new();
        let mut byte = [0u8; 1];
        let mut broken = false;
        while !head.ends_with(b"\r\n\r\n") {
            match stream.read(&mut byte) {
                Ok(1) => head.push(byte[0]),
                _ => {
                    broken = true;
                    break;
                }
            }
        }
        if broken {
            continue;
        }
        let lowered = String::from_utf8_lossy(&head).to_ascii_lowercase();
        let length = lowered
            .lines()
            .find_map(|line| line.strip_prefix("content-length:"))
            .and_then(|value| value.trim().parse::<usize>().ok())
            .unwrap_or(0);
        let mut body = vec![0u8; length];
        if stream.read_exact(&mut body).is_err() {
            continue;
        }
        let objects: Vec<String> = requested(&String::from_utf8_lossy(&body))
            .into_iter()
            .map(|(oid, size)| {
                if oid == lost {
                    format!(
                        "{{\"oid\":\"{oid}\",\"size\":{size},\
                         \"error\":{{\"code\":404,\"message\":\"absent\"}}}}"
                    )
                } else {
                    format!(
                        "{{\"oid\":\"{oid}\",\"size\":{size},\
                         \"actions\":{{\"download\":{{\"href\":\"http://127.0.0.1:1/o\"}}}}}}"
                    )
                }
            })
            .collect();
        let answer = format!(
            "{{\"transfer\":\"basic\",\"objects\":[{}]}}",
            objects.join(",")
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

/// The `(oid, size)` pairs a batch request asked about.
fn requested(body: &str) -> Vec<(String, u64)> {
    let mut out = Vec::new();
    let mut rest = body;
    while let Some(at) = rest.find("\"oid\":\"") {
        rest = &rest[at + "\"oid\":\"".len()..];
        let Some(end) = rest.find('"') else {
            break;
        };
        let oid = rest[..end].to_owned();
        rest = &rest[end..];
        let Some(sized) = rest.find("\"size\":") else {
            break;
        };
        rest = &rest[sized + "\"size\":".len()..];
        let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
        let Ok(size) = digits.parse::<u64>() else {
            break;
        };
        out.push((oid, size));
    }
    out
}

/// A profile whose remote is an answering LFS server, with `lost`'s object the
/// one hole in it.
fn with_batch_server(root: &Path, remote: &Path, lost: &Pointer) -> SyncProfile {
    let listening = std::net::TcpListener::bind("127.0.0.1:0").expect("bind loopback");
    let port = listening.local_addr().expect("addr").port();
    serve_batches(listening, lost.oid.clone());
    let mut asked = profile(root, remote);
    // An answering server rather than the filesystem remote: `audit_remote_objects`
    // reports a filesystem remote intact without asking anything (a recorded
    // deferred finding), so a fixture that used one would prove nothing about
    // the half that condemns.
    asked.remote_url = format!("http://127.0.0.1:{port}/r.git");
    asked
}

// ---------------------------------------------------------------------------
// verify: the offline half stops condemning what it cannot know
// ---------------------------------------------------------------------------

/// NFR-41's own shape: a folder whose policy leaves a thousand committed
/// pointers virtual reports **nothing**, and says how many there are.
///
/// A thousand rather than one, deliberately. The failure mode this story could
/// create is a check that went quiet, and no fixture of one path can tell the
/// difference between "excused correctly" and "stopped looking".
#[tokio::test]
async fn a_thousand_authorized_virtual_paths_are_not_a_fault() {
    let work = tempfile::tempdir().expect("tempdir");
    let root = work.path();
    let remote = tempfile::tempdir().expect("remote");
    let Some(seeded) = seed(root, remote.path(), 1000) else {
        return;
    };
    let data = tempfile::tempdir().expect("data dir");
    let engine = engine_for(root, remote.path(), data.path());

    let report = engine.verify(PROFILE_ID).await.expect("verify");

    assert_eq!(
        report.bad,
        Vec::<(String, String)>::new(),
        "the normal state of a folder that keeps pointers is not damage"
    );
    assert_eq!(
        report.virtual_paths, 1000,
        "every excused path is counted; a suppressed row nobody reports is \
         indistinguishable from a check that stopped running"
    );
    assert!(
        report.checked > seeded.virtual_paths.len() as u64,
        "`checked` still counts every file, the excused ones included: {}",
        report.checked
    );
}

/// One pointer the policy does not authorize, standing in the same folder as a
/// thousand it does, is the **only** row reported.
#[tokio::test]
async fn the_unauthorized_path_is_the_only_row_reported() {
    let work = tempfile::tempdir().expect("tempdir");
    let root = work.path();
    let remote = tempfile::tempdir().expect("remote");
    let Some(_seeded) = seed(root, remote.path(), 1000) else {
        return;
    };
    let lost = add_unauthorized(root, remote.path());
    let data = tempfile::tempdir().expect("data dir");
    let engine = engine_for(root, remote.path(), data.path());

    let report = engine.verify(PROFILE_ID).await.expect("verify");

    assert_eq!(
        report.bad,
        vec![(
            UNAUTHORIZED.to_owned(),
            format!("LFS object {} is missing locally", lost.oid)
        )],
        "the surviving report, word for word as before"
    );
    assert_eq!(report.virtual_paths, 1000);
}

/// The **index** arm. A pointer-shaped file the index does not carry is a guess
/// about somebody's bytes, however well the patterns match its path.
#[tokio::test]
async fn a_pointer_shaped_file_the_index_does_not_carry_is_still_reported() {
    let work = tempfile::tempdir().expect("tempdir");
    let root = work.path();
    let remote = tempfile::tempdir().expect("remote");
    let Some(_seeded) = seed(root, remote.path(), 4) else {
        return;
    };
    // Inside the authorized zone and matched by the patterns, but never
    // committed: the state a user produces by saving a text file, or a half-
    // finished checkout leaves behind.
    let stray = seed_object(&LfsStore::in_git_dir(remote.path()), &payload(9_999));
    std::fs::write(root.join(format!("{ZONE}/stray.mp4")), stray.render()).expect("write");

    let data = tempfile::tempdir().expect("data dir");
    let engine = engine_for(root, remote.path(), data.path());
    let report = engine.verify(PROFILE_ID).await.expect("verify");

    assert_eq!(
        report.bad,
        vec![(
            format!("{ZONE}/stray.mp4"),
            format!("LFS object {} is missing locally", stray.oid)
        )],
        "the policy authorizes a PATH; only the index can say the bytes on disk \
         are the pointer this repository committed"
    );
    assert_eq!(
        report.virtual_paths, 4,
        "the committed four are still excused"
    );
}

/// A worktree pointer whose oid is not the committed one is a different file,
/// and the index disagreeing is exactly what says so.
#[tokio::test]
async fn a_worktree_pointer_that_disagrees_with_the_index_is_still_reported() {
    let work = tempfile::tempdir().expect("tempdir");
    let root = work.path();
    let remote = tempfile::tempdir().expect("remote");
    let Some(seeded) = seed(root, remote.path(), 4) else {
        return;
    };
    let other = seed_object(&LfsStore::in_git_dir(remote.path()), &payload(8_888));
    let swapped = seeded.virtual_paths[0].clone();
    std::fs::write(root.join(&swapped), other.render()).expect("swap the pointer");

    let data = tempfile::tempdir().expect("data dir");
    let engine = engine_for(root, remote.path(), data.path());
    let report = engine.verify(PROFILE_ID).await.expect("verify");

    assert_eq!(
        report.bad,
        vec![(
            swapped,
            format!("LFS object {} is missing locally", other.oid)
        )]
    );
    assert_eq!(report.virtual_paths, 3);
}

/// No repository, no index, therefore no proof — and therefore no excuse, for
/// any path.
#[tokio::test]
async fn a_folder_with_no_repository_excuses_nothing() {
    let work = tempfile::tempdir().expect("tempdir");
    let root = work.path();
    let remote = tempfile::tempdir().expect("remote");
    let remote_store = LfsStore::in_git_dir(remote.path());
    std::fs::create_dir_all(root.join(ZONE)).expect("zone");
    std::fs::write(root.join(VIRTUAL_PATTERN_FILE), format!("{ZONE}/**\n")).expect("policy");
    let mut expected = Vec::new();
    for n in 0..4 {
        let rela = format!("{ZONE}/clip-{n:05}.mp4");
        let pointer = seed_object(&remote_store, &payload(n));
        std::fs::write(root.join(&rela), pointer.render()).expect("write");
        expected.push((
            rela,
            format!("LFS object {} is missing locally", pointer.oid),
        ));
    }

    let data = tempfile::tempdir().expect("data dir");
    let engine = engine_for(root, remote.path(), data.path());
    let mut report = engine.verify(PROFILE_ID).await.expect("verify");
    report.bad.sort();
    expected.sort();

    assert_eq!(
        report.bad, expected,
        "a matching policy with nothing to check it against is not an authorization"
    );
    assert_eq!(report.virtual_paths, 0);
}

/// A policy that cannot be compiled fails the verb, and never quietly excuses
/// or quietly condemns (FR-329) — and leaves the folder idle.
///
/// The phase assertion is Story 34.8's, and it is why the compile happens
/// inside the blocking closure rather than before it. `verify` takes no
/// reservation, so `Reservation::drop` cannot clean up after it: any `?`
/// between the `Verifying` publish and the `Idle` publish at the tail leaves a
/// spinner on the tray and a bar on the card until the process ends, and a typo
/// in a committed file is a very easy way for a user to reach one.
#[tokio::test]
async fn a_malformed_policy_makes_verify_refuse_before_the_walk() {
    let work = tempfile::tempdir().expect("tempdir");
    let root = work.path();
    let remote = tempfile::tempdir().expect("remote");
    let Some(_seeded) = seed(root, remote.path(), 2) else {
        return;
    };
    std::fs::write(root.join(VIRTUAL_PATTERN_FILE), "40-media/[unclosed\n").expect("break it");

    let data = tempfile::tempdir().expect("data dir");
    let engine = engine_for(root, remote.path(), data.path());
    let err = engine
        .verify(PROFILE_ID)
        .await
        .expect_err("a policy that cannot be read is not a permissive policy");

    assert!(
        matches!(&err, SyncError::Config(message) if message.contains("40-media/[unclosed")),
        "the refusal must quote the line as typed: {err}"
    );
    assert_eq!(
        engine
            .status(PROFILE_ID)
            .expect("the profile still has a status")
            .phase,
        SyncPhase::Idle,
        "a refused verify must put the folder back, or the card spins forever"
    );
}

/// The **third fact**, and without it neither half of the check speaks for an
/// external drive.
///
/// `audit_remote_objects` short-circuits a filesystem remote to `missing: []`
/// without asking it anything, so once the offline half stops condemning an
/// authorized virtual path, an object that vanished from the drive is reported
/// by NOBODY. Here the drive is plugged in and one object is gone from it: that
/// path's content exists nowhere, and it is the only row.
#[tokio::test]
async fn a_virtual_path_whose_object_left_the_remote_store_is_still_reported() {
    let work = tempfile::tempdir().expect("tempdir");
    let root = work.path();
    let remote = tempfile::tempdir().expect("remote");
    let Some(seeded) = seed(root, remote.path(), 4) else {
        return;
    };
    let lost = seeded.pointers[2].clone();
    std::fs::remove_file(LfsStore::in_git_dir(remote.path()).object_path(&lost.oid))
        .expect("take the object off the drive");

    let data = tempfile::tempdir().expect("data dir");
    let engine = engine_for(root, remote.path(), data.path());
    let report = engine.verify(PROFILE_ID).await.expect("verify");

    assert_eq!(
        report.bad,
        vec![(
            seeded.virtual_paths[2].clone(),
            format!("LFS object {} is missing locally", lost.oid)
        )],
        "the drive is right there and does not hold it, so this is loss and \
         nothing else will say so"
    );
    assert_eq!(
        report.virtual_paths, 3,
        "and the three the drive still holds stay excused"
    );
}

/// The drive is **out**, so the third fact cannot be taken and must not be read
/// as a no.
///
/// Absence is never failure (AD-48). A folder on a pendrive that is not plugged
/// in would otherwise report every virtual path it has the moment somebody ran
/// the scheduled check, which is the loudest possible way to say nothing.
#[tokio::test]
async fn an_unplugged_remote_leaves_the_excuse_to_the_facts_on_this_disk() {
    let work = tempfile::tempdir().expect("tempdir");
    let root = work.path();
    let remote = tempfile::tempdir().expect("remote");
    let Some(_seeded) = seed(root, remote.path(), 4) else {
        return;
    };
    // The drive comes out. `TempDir`'s own cleanup tolerates this.
    std::fs::remove_dir_all(remote.path()).expect("unplug the drive");

    let data = tempfile::tempdir().expect("data dir");
    let engine = engine_for(root, remote.path(), data.path());
    let report = engine.verify(PROFILE_ID).await.expect("verify");

    assert_eq!(
        report.bad,
        Vec::<(String, String)>::new(),
        "a store nobody could ask has not answered no"
    );
    assert_eq!(report.virtual_paths, 4);
}

/// A folder whose `lfsMode` is `disabled` excuses nothing, however well its
/// policy matches.
///
/// With LFS off, nothing in the engine will ever materialize one of these
/// paths: no pass consults the store for it, and `materialize_entry` refuses
/// outright. Calling that content "deliberately elsewhere" would be a promise
/// nothing in the product keeps.
#[tokio::test]
async fn a_folder_with_lfs_disabled_excuses_nothing() {
    let work = tempfile::tempdir().expect("tempdir");
    let root = work.path();
    let remote = tempfile::tempdir().expect("remote");
    let Some(seeded) = seed(root, remote.path(), 4) else {
        return;
    };
    let mut disabled = profile(root, remote.path());
    disabled.lfs_mode = LfsMode::Disabled;

    let data = tempfile::tempdir().expect("data dir");
    let engine = engine_with(disabled, data.path());
    let mut report = engine.verify(PROFILE_ID).await.expect("verify");
    report.bad.sort();
    let mut expected: Vec<(String, String)> = seeded
        .virtual_paths
        .iter()
        .zip(&seeded.pointers)
        .map(|(path, pointer)| {
            (
                path.clone(),
                format!("LFS object {} is missing locally", pointer.oid),
            )
        })
        .collect();
    expected.sort();

    assert_eq!(report.bad, expected);
    assert_eq!(report.virtual_paths, 0);
}

/// An object sitting here at the **wrong length** is damage, and damage stays
/// reported.
///
/// `LfsStore::contains` is length-verified precisely because a `kill -9` during
/// a write, or a half-restored backup, leaves a correctly-named file holding a
/// prefix of the content. So a truncated object answers "not here" — and
/// excusing it as virtual would silence the exact failure that check exists to
/// catch. Only a genuinely absent object is the normal state of a virtual path.
#[tokio::test]
async fn an_object_present_at_the_wrong_length_stays_reported() {
    let work = tempfile::tempdir().expect("tempdir");
    let root = work.path();
    let remote = tempfile::tempdir().expect("remote");
    let Some(seeded) = seed(root, remote.path(), 4) else {
        return;
    };
    let truncated = seeded.pointers[1].clone();
    let object = LfsStore::in_git_dir(root.join(".git")).object_path(&truncated.oid);
    std::fs::create_dir_all(object.parent().expect("the shard directories")).expect("shards");
    std::fs::write(&object, b"a prefix of the content and nothing more").expect("truncate");

    let data = tempfile::tempdir().expect("data dir");
    let engine = engine_for(root, remote.path(), data.path());
    let report = engine.verify(PROFILE_ID).await.expect("verify");

    assert_eq!(
        report.bad,
        vec![(
            seeded.virtual_paths[1].clone(),
            format!("LFS object {} is missing locally", truncated.oid)
        )],
        "a short blob under the right name is not an absent object"
    );
    assert_eq!(report.virtual_paths, 3);
}

// ---------------------------------------------------------------------------
// verify --remote: the half that still condemns
// ---------------------------------------------------------------------------

/// **The negative direction.** With the offline half quiet about authorized
/// virtual paths, `--remote` is the only check left that can condemn one — so a
/// change that silenced the checks entirely would pass every test above and
/// fail this.
#[tokio::test]
async fn verify_remote_still_reports_a_virtual_path_the_server_cannot_serve() {
    let work = tempfile::tempdir().expect("tempdir");
    let root = work.path();
    let remote = tempfile::tempdir().expect("remote");
    let Some(seeded) = seed(root, remote.path(), 3) else {
        return;
    };
    let lost = seeded.pointers[1].clone();
    let asked = with_batch_server(root, remote.path(), &lost);
    let data = tempfile::tempdir().expect("data dir");
    let engine = engine_with(asked, data.path());

    // The offline half is quiet, as it must be: nothing on this disk
    // distinguishes the lost object from the two intact ones.
    let report = engine.verify(PROFILE_ID).await.expect("verify");
    assert_eq!(report.bad, Vec::<(String, String)>::new());
    assert_eq!(report.virtual_paths, 3);

    let audit = engine
        .audit_remote_objects(PROFILE_ID)
        .await
        .expect("audit the remote");

    assert_eq!(
        audit
            .missing
            .iter()
            .map(|object| (object.path.clone(), object.oid.clone(), object.code))
            .collect::<Vec<_>>(),
        vec![(seeded.virtual_paths[1].clone(), lost.oid.clone(), 404)],
        "an authorized virtual path whose object the server does not hold is \
         still real, permanent loss"
    );
    assert!(!audit.is_intact(), "and the exit gate reads exactly this");
    assert_eq!(audit.missing_bytes(), lost.size);
}

/// A repair pass over a virtual path the server lacks reports it and **writes
/// nothing**.
///
/// The store's object set is enumerated on both sides, which asserts the write
/// did not happen rather than that the answer came out right: `stage::clean`
/// streams the file into the store before it returns, so a gate placed around
/// its answer would leave a junk object under the pointer text's own sha —
/// named by no pointer, and never collected by `prune`.
#[tokio::test]
async fn a_repair_pass_over_a_virtual_path_adds_no_object_to_the_store() {
    let work = tempfile::tempdir().expect("tempdir");
    let root = work.path();
    let remote = tempfile::tempdir().expect("remote");
    let Some(seeded) = seed(root, remote.path(), 3) else {
        return;
    };
    let lost = seeded.pointers[2].clone();
    let asked = with_batch_server(root, remote.path(), &lost);
    let data = tempfile::tempdir().expect("data dir");
    let engine = engine_with(asked, data.path());

    let git_dir = root.join(".git");
    LfsStore::in_git_dir(&git_dir)
        .ensure_layout()
        .expect("layout");
    let before = store_objects(&git_dir);

    let repaired = engine
        .republish_missing_objects(PROFILE_ID)
        .await
        .expect("repair");

    assert_eq!(
        repaired
            .unrecoverable
            .iter()
            .map(|object| object.path.clone())
            .collect::<Vec<_>>(),
        vec![seeded.virtual_paths[2].clone()],
        "content that is on neither the server nor this machine is beyond this \
         machine, whatever the reason the bytes are not here"
    );
    assert_eq!(repaired.queued, 0, "nothing here can be re-uploaded");
    assert_eq!(
        store_objects(&git_dir),
        before,
        "re-cleaning the pointer text would have deposited a junk object under \
         its own sha, which nothing names and nothing collects"
    );
}

/// The repair still repairs: a worktree file that IS the content, whose object
/// this machine's store no longer holds, is re-cleaned and queued.
///
/// The matrix's own row, and the direction the new gate could have killed
/// silently. This is the ordinary state on the machine that authored the file
/// after a `prune` — the bytes are right there, the object is not, and hashing
/// the file back in is the whole point of the verb.
#[tokio::test]
async fn a_worktree_that_holds_the_content_is_re_cleaned_and_queued() {
    let work = tempfile::tempdir().expect("tempdir");
    let root = work.path();
    let remote = tempfile::tempdir().expect("remote");
    let Some(seeded) = seed(root, remote.path(), 1) else {
        return;
    };
    // The authoring machine: the worktree file is the file, not its pointer.
    let content = payload(0);
    std::fs::write(root.join(&seeded.virtual_paths[0]), &content).expect("hold the content");
    let lost = seeded.pointers[0].clone();
    let asked = with_batch_server(root, remote.path(), &lost);
    let data = tempfile::tempdir().expect("data dir");
    let engine = engine_with(asked, data.path());

    let store = LfsStore::in_git_dir(root.join(".git"));
    assert!(
        !store.contains(&lost.oid, lost.size),
        "the object has to be absent for the repair to have anything to do"
    );

    let repaired = engine
        .republish_missing_objects(PROFILE_ID)
        .await
        .expect("repair");

    assert_eq!(repaired.queued, 1, "the bytes are here and re-uploadable");
    assert!(
        repaired.unrecoverable.is_empty(),
        "nothing here is beyond this machine: {:?}",
        repaired.unrecoverable
    );
    assert!(
        store.contains(&lost.oid, lost.size),
        "the re-clean must have put the object back, or there is nothing to \
         upload when the unit runs"
    );
}

/// A pointer-shaped worktree file naming a **different** oid is still
/// re-cleaned.
///
/// The gate is "the pointer naming *this very object*", not "anything that
/// looks like a pointer", and the difference is a real and recoverable shape: a
/// tracked file whose genuine content IS pointer text — this project's own LFS
/// documentation, a fixture holding an example pointer — under a small
/// `lfsThresholdBytes`. Its committed pointer names the sha of that text, and
/// re-cleaning it produces exactly that oid. A gate on `is_some()` alone would
/// report the file as beyond this machine, with the loudest warning the product
/// prints, about a file sitting right there.
#[tokio::test]
async fn a_worktree_pointer_naming_another_object_is_re_cleaned_and_queued() {
    let work = tempfile::tempdir().expect("tempdir");
    let root = work.path();
    let remote = tempfile::tempdir().expect("remote");
    let Some(seeded) = seed(root, remote.path(), 1) else {
        return;
    };
    let remote_store = LfsStore::in_git_dir(remote.path());
    // The file's genuine content is a pointer for something else entirely.
    let documented = seed_object(&remote_store, &payload(31));
    let genuine = documented.render().into_bytes();
    // ...so its own committed pointer names the sha of that text.
    let committed = seed_object(&remote_store, &genuine);

    let path = format!("{ZONE}/pointer-example.md");
    std::fs::write(root.join(&path), committed.render()).expect("check out the pointer");
    let repo = git::repo::open(root, false).expect("reopen");
    commit(&repo, remote.path(), vec![PathBuf::from(&path)]);
    drop(repo);
    std::fs::write(root.join(&path), &genuine).expect("the content is pointer text");

    let asked = with_batch_server(root, remote.path(), &committed);
    let data = tempfile::tempdir().expect("data dir");
    let engine = engine_with(asked, data.path());
    let store = LfsStore::in_git_dir(root.join(".git"));

    let repaired = engine
        .republish_missing_objects(PROFILE_ID)
        .await
        .expect("repair");

    assert_eq!(
        repaired
            .unrecoverable
            .iter()
            .map(|object| object.path.clone())
            .collect::<Vec<_>>(),
        Vec::<String>::new(),
        "recoverable content must not be reported as loss"
    );
    assert_eq!(repaired.queued, 1);
    assert!(
        store.contains(&committed.oid, committed.size),
        "the pointer text hashed back in under the oid its own pointer names"
    );
    // The seeded path is untouched by any of this, and stays what it was.
    assert!(!store.contains(&seeded.pointers[0].oid, seeded.pointers[0].size));
}

// ---------------------------------------------------------------------------
// The verified copy
// ---------------------------------------------------------------------------

/// End to end through a real `Engine`, a real index and a real object store:
/// the destination gets the file's bytes, not its pointer.
#[tokio::test]
async fn a_verified_copy_with_an_engine_carries_the_real_bytes() {
    let work = tempfile::tempdir().expect("tempdir");
    let root = work.path();
    let remote = tempfile::tempdir().expect("remote");
    let Some(seeded) = seed(root, remote.path(), 1) else {
        return;
    };
    // The object IS on this machine — the case where hydration can succeed
    // without waiting for anything.
    let content = payload(0);
    seed_object(&LfsStore::in_git_dir(root.join(".git")), &content);

    let data = tempfile::tempdir().expect("data dir");
    let engine = engine_for(root, remote.path(), data.path());
    // Its own tempdir, so a destination is never shared between runs and a
    // failing assertion cannot leave a stub behind for the next one to find.
    let target = tempfile::tempdir().expect("target");
    let destination = target.path().join("dst");
    let source = root.join(ZONE);
    let report = copy_verified(
        &source,
        &destination,
        &CopyOptions::default(),
        None,
        &off(),
        Some(&engine as &dyn ContentSource),
    )
    .expect("copy");

    let name = seeded.virtual_paths[0]
        .strip_prefix(&format!("{ZONE}/"))
        .expect("zone-relative");
    let copied = report
        .entries
        .iter()
        .find(|entry| entry.path == name)
        .unwrap_or_else(|| panic!("no entry for {name}: {:?}", report.entries));
    assert_eq!(copied.outcome, CopyOutcome::Copied);
    assert_eq!(
        std::fs::read(destination.join(name)).expect("read the destination"),
        content,
        "the destination must hold the file, byte for byte — not the ~130 bytes \
         that stand in for it"
    );
    assert_eq!(copied.bytes, content.len() as u64);
}

/// The owning folder is found by CANONICAL path, so a source reached through a
/// symlinked ancestor still resolves to it (AD-59).
///
/// Not an exotic case: `/tmp` and `/var` are symlinks on macOS and a relocated
/// home directory is one everywhere. A lexical `strip_prefix` answers
/// `NotTracked` here — a sentence the user knows to be false — and every large
/// file in the folder then silently fails to reach the pendrive.
#[tokio::test]
async fn a_copy_reached_through_a_symlinked_ancestor_still_finds_its_folder() {
    let work = tempfile::tempdir().expect("tempdir");
    let root = work.path();
    let remote = tempfile::tempdir().expect("remote");
    let Some(seeded) = seed(root, remote.path(), 1) else {
        return;
    };
    let content = payload(0);
    seed_object(&LfsStore::in_git_dir(root.join(".git")), &content);

    let data = tempfile::tempdir().expect("data dir");
    let engine = engine_for(root, remote.path(), data.path());

    let elsewhere = tempfile::tempdir().expect("elsewhere");
    let link = elsewhere.path().join("folder");
    std::os::unix::fs::symlink(root, &link).expect("reach the folder through a symlink");

    let target = tempfile::tempdir().expect("target");
    let destination = target.path().join("dst");
    let report = copy_verified(
        &link.join(ZONE),
        &destination,
        &CopyOptions::default(),
        None,
        &off(),
        Some(&engine as &dyn ContentSource),
    )
    .expect("copy");

    let name = seeded.virtual_paths[0]
        .strip_prefix(&format!("{ZONE}/"))
        .expect("zone-relative");
    let copied = report
        .entries
        .iter()
        .find(|entry| entry.path == name)
        .unwrap_or_else(|| panic!("no entry for {name}: {:?}", report.entries));
    assert_eq!(
        copied.outcome,
        CopyOutcome::Copied,
        "the folder is right there under another name for the same directory"
    );
    assert_eq!(
        std::fs::read(destination.join(name)).expect("read the destination"),
        content
    );
}

/// The object is on neither the server nor here, so nothing can hand the copy
/// its bytes — and the refusal must cost the user nothing.
///
/// Two claims, and the second is the one that was missing. `materialize_entry`
/// answers `Queued` only *after* it has enqueued a download, labelled it,
/// promoted it and woken the supervisor — and `wake_now` also resets 56.5's
/// release rotation. So a copy of a folder that holds nothing but pointers
/// would start fetching the whole zone the user never asked for, as a side
/// effect of being told no. The journal is enumerated afterwards to prove it
/// did not.
#[tokio::test]
async fn a_copy_of_a_path_whose_object_is_not_here_refuses_by_name() {
    let work = tempfile::tempdir().expect("tempdir");
    let root = work.path();
    let remote = tempfile::tempdir().expect("remote");
    let Some(seeded) = seed(root, remote.path(), 1) else {
        return;
    };
    let data = tempfile::tempdir().expect("data dir");
    let engine = engine_for(root, remote.path(), data.path());
    let target = tempfile::tempdir().expect("target");
    let destination = target.path().join("dst");
    let name = seeded.virtual_paths[0]
        .strip_prefix(&format!("{ZONE}/"))
        .expect("zone-relative")
        .to_owned();

    let report = copy_verified(
        &root.join(ZONE),
        &destination,
        &CopyOptions::default(),
        None,
        &off(),
        Some(&engine as &dyn ContentSource),
    )
    .expect("a refusal is one entry, never the end of the job");

    assert_eq!(
        failure(&report, &name),
        ContentRefusal::ContentNotHere { path: name.clone() }.to_string(),
        "the row is keyed by the copy-relative path, so the sentence names that \
         path too — one file must not be given two names in one line"
    );
    assert!(
        !destination.join(&name).exists(),
        "a ~130-byte stub reached the destination"
    );
    let status = engine.status(PROFILE_ID).expect("status");
    assert_eq!(
        (status.pending, status.queued_files),
        (0, 0),
        "a refused copy must not have committed this machine to downloading the \
         object it refused: {status:?}"
    );
}

/// A pointer-shaped file under no folder keeper syncs refuses by name too: the
/// engine has no repository to ask.
#[tokio::test]
async fn a_copy_of_a_pointer_outside_every_profile_refuses_by_name() {
    let work = tempfile::tempdir().expect("tempdir");
    let root = work.path();
    let remote = tempfile::tempdir().expect("remote");
    let Some(_seeded) = seed(root, remote.path(), 1) else {
        return;
    };
    let data = tempfile::tempdir().expect("data dir");
    let engine = engine_for(root, remote.path(), data.path());

    let outside = tempfile::tempdir().expect("outside");
    let stray = seed_object(&LfsStore::in_git_dir(remote.path()), &payload(4_242));
    std::fs::write(outside.path().join("clip.mp4"), stray.render()).expect("write");
    let target = tempfile::tempdir().expect("target");
    let destination = target.path().join("dst");

    let report = copy_verified(
        outside.path(),
        &destination,
        &CopyOptions::default(),
        None,
        &off(),
        Some(&engine as &dyn ContentSource),
    )
    .expect("copy");

    assert_eq!(
        failure(&report, "clip.mp4"),
        ContentRefusal::NotTracked {
            path: outside
                .path()
                .join("clip.mp4")
                .to_string_lossy()
                .into_owned(),
        }
        .to_string()
    );
}
