//! Story 31.2 — the NFR-24 durability matrix, indexed by **stage boundary**.
//!
//! NFR-24 says that a fault at any instant either completes the operation or
//! lets the next run resume cleanly, and that no cell may lose a byte or leave
//! a corrupt index. This file asserts both halves at each of the five stage
//! boundaries a sync passes through — worktree → index (*stage*), index →
//! commit (*commit*), local → remote (*push*), local → LFS server (*LFS
//! upload*), and remote → worktree (*checkout*) — against the `keeper-sync`
//! API that owns each boundary.
//!
//! # The other half of the matrix
//!
//! `keeper-syncd/tests/durability_matrix.rs` is the daemon-level half: it
//! spawns the real daemon and SIGKILLs it at a dense sweep of instants across a
//! whole sync, recording what each kill actually interrupted. That sweep is the
//! only way to reach states nothing can stage on purpose, and it is where the
//! two production repairs this file leans on were found — `repo.rs:892`
//! (a `.git` left without its remote) and `repo.rs:999` (a `.git` left without
//! its `origin` config).
//!
//! It is also, by construction, a *distribution* over instants rather than a
//! grid: which stage a given kill lands in is a property of the machine. So it
//! can never say "the commit boundary is covered", only "the sweep reached the
//! commit boundary this time". This file is the deterministic complement — one
//! named cell per boundary, hitting that boundary every run, on every machine.
//!
//! # The grid, and why it has holes
//!
//! Three fault kinds are meaningful here: a `kill -9` of the writing process, a
//! removable volume disappearing under it (AD-48), and network loss. They are
//! not all meaningful at every boundary, and a cell that cannot fail is a test
//! that asserts nothing, so the grid is deliberately sparse:
//!
//! | boundary | `kill -9` | volume disappears | network loss |
//! | --- | --- | --- | --- |
//! | **stage** (worktree → index) | [`a_kill_during_staging_loses_no_committed_file_and_leaves_a_usable_index`] | [`a_volume_pulled_during_staging_is_never_a_deletion_and_loses_no_committed_file`] | hole (1) |
//! | **commit** (index → commit) | [`a_commit_interrupted_between_the_index_and_the_reference_resumes_cleanly`] | hole (2) | hole (1) |
//! | **push** | hole (3) | hole (2) | [`a_push_cut_off_by_network_loss_keeps_its_journal_unit_and_publishes_on_the_retry`] |
//! | **LFS upload** | hole (3) | hole (2) | [`an_lfs_upload_cut_off_mid_body_stores_no_partial_object_and_re_sends_every_byte`] |
//! | **checkout** (remote → worktree) | [`a_checkout_interrupted_while_it_held_the_index_applies_the_remote_change_on_the_retry`] | hole (4) | hole (1) |
//!
//! **Hole (1) — no network at this boundary.** Staging, committing and
//! checking out are pure local filesystem work: staging reads the worktree and
//! writes `.git/index`, committing writes objects and one reference, and a
//! checkout applies objects that a *previous* fetch already brought down. There
//! is no socket open in any of them, so "network loss" has nothing to cut. A
//! cell here could only assert that an unrelated operation was unaffected.
//!
//! **Hole (2) — indistinguishable from the kill in the same row.** An unplug
//! during a `.git`-internal transaction is, from the repository's point of
//! view, exactly the kill cell beside it: the writer stops between two
//! filesystem operations and leaves the same debris (a written index, no
//! reference update, possibly a lock). The part that *is* specific to an
//! unplug — that an absent volume must never read as a deletion, and that the
//! work survives the absence — needs the volume to actually go away and come
//! back, which is what the stage row's unplug cell does. Repeating it per row
//! would re-assert one invariant four times.
//!
//! **Hole (3) — owned by the daemon-level half.** A kill mid-push and a kill
//! mid-LFS-upload are precisely the two states in-process fault injection
//! cannot reach honestly, because the interesting part is what the *supervisor*
//! does with a journal unit whose process died. Both are covered where the
//! supervisor exists: `keeper-syncd/tests/durability_matrix.rs:533`
//! (`a_kill_leaves_no_committed_work_unpublished_forever`) and
//! `keeper-syncd/tests/durability_matrix.rs:450`
//! (`a_kill_during_a_large_object_transfer_leaves_the_object_recoverable`).
//! Restaging them here would be a second, weaker copy.
//!
//! **Hole (4) — a checkout writes only the worktree.** An unplug mid-checkout
//! leaves a half-applied worktree on media that is gone; recovering it is the
//! *stage* row's fault at a different moment, and the deciding invariant — the
//! absent volume is not a deletion — is asserted there. What a checkout must
//! not do, which is corrupt the index while it holds it, is the kill cell in
//! this row.
//!
//! # What every implemented cell asserts
//!
//! Both halves of the AC, never one:
//!
//! * **(a) completed, or the next run resumes cleanly.** Never "it did not
//!   panic": the recovery is driven for real and the operation is then required
//!   to reach its intended end state.
//! * **(b) no byte lost, no corrupt index.** Content is compared
//!   byte-for-byte — in the worktree, in the committed tree, and for LFS on the
//!   wire — and the repository is put through `git fsck --full --strict` plus a
//!   `git status` that fails outright on an unreadable index. This is git's own
//!   integrity check, not ours.
//!
//! # Deliberate fixture choices
//!
//! **Planted faults where a race would prove less.** Three cells plant the
//! exact artifact a kill leaves — an abandoned `HEAD.lock`, an abandoned
//! `index.lock` — rather than racing a signal into a window microseconds wide.
//! This is the same trade `keeper-syncd`'s `plant_head_lock` makes and for the
//! same reason: the sweep proves the states occur, and a planted state proves
//! the folder digs itself out of them every single run. The two cells where the
//! *timing* is the point still use a real SIGKILL of a real child process.
//!
//! **Every scratch directory is `tempfile::tempdir()`.** Commit `c28e63d` fixed
//! six helpers that named a temp directory from the pid plus a nanosecond
//! stamp: two threads asking inside one clock tick got the same name and
//! collided. `mkdtemp` is that rule enforced by the kernel — unique per call by
//! construction, never per clock tick — and it is what `keeper-sync`'s existing
//! integration tests already use. Nothing here uses a fixed path or a fixed
//! port; every socket binds port 0 and reads back what it got, so the whole
//! file is safe under a shared `cargo test --workspace` runner.
//!
//! **No test may outlive itself.** Every spawned child is owned by a
//! [`KilledChild`] guard that SIGKILLs and reaps on drop, including on an
//! assertion unwind, and every stub server is a thread that dies with the
//! process. A failing cell leaves nothing behind.
//!
//! **Runtime intent: about five seconds for the file.** The repo's perf gates
//! already cost ~20 s and this must not add a coffee break. Four cells are
//! sub-second. The two that are not are paying real production waits, not
//! sleeps of our own: the commit cell waits out `repo.rs`'s ~2 s reference-lock
//! watch, and the two kill cells may wait it out again if the kill happened to
//! land inside a reference transaction. Everything else is bounded by
//! [`CHILD_DEADLINE`], which exists so a wedged child fails diagnosably instead
//! of hanging a reliability gate.

use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

use keeper_sync::db::{self, WorkKind, WorkState};
use keeper_sync::git::{self, cli::GitCli, commit::StagedChange};
use keeper_sync::lfs::basic::BasicTransfer;
use keeper_sync::lfs::batch::{Action, Actions, ObjectSpec};
use keeper_sync::lfs::store::LfsStore;
use keeper_sync::profile::SyncProfile;
use keeper_sync::provenance::{Provenance, SyncSource};
use keeper_sync::volume::{self, VolumeMarker, VolumeStatus};
use keeper_sync::SyncError;
use sha2::{Digest, Sha256};

/// Switches [`durability_child_stages_until_killed`] from a no-op into a
/// staging writer. Its value is the profile folder to stage into.
///
/// The child is an ordinary `#[test]` in this same binary, re-invoked through
/// `current_exe()`, so it must be inert on a normal run or the suite would
/// recurse forever. Reading the variable is the whole switch.
const DURABILITY_CHILD_ENV: &str = "KEEPER_SYNC_DURABILITY_CHILD";

/// How many commits the parent watches land before it pulls the trigger.
///
/// Small on purpose. The kill has to arrive while the child is still working,
/// and every extra commit is time spent proving nothing new: eight is enough
/// for the "previously committed work survives" assertion to have teeth and
/// lands the SIGKILL a few milliseconds into the run.
const KILL_AFTER: usize = 8;

/// Wall-clock budget for one line from the child.
///
/// A healthy child emits eight commits in milliseconds, so only a genuinely
/// stuck one reaches this. It exists because a *reliability* gate that can hang
/// is worse than one that fails: this turns a hypothetical infinite wait into a
/// bounded failure that names the child.
const CHILD_DEADLINE: Duration = Duration::from_secs(30);

/// Big enough that the socket buffers cannot swallow the whole body.
///
/// The mid-upload cell needs the peer to vanish with bytes genuinely in flight.
/// A payload that fits in the kernel's send buffer would be handed over
/// complete before the drop and the client would fail on the *response* instead
/// of on the transfer — a different fault wearing the same name. Four MiB is
/// comfortably past any default loopback buffer while still costing nothing to
/// generate or compare.
const LFS_PAYLOAD_BYTES: usize = 4 * 1024 * 1024;

// ---------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------

/// Deterministic content for staged file `i`, reproducible by the parent from
/// the index alone — which is what lets a byte-for-byte comparison happen after
/// the writer that produced it has been killed.
fn payload(i: usize) -> Vec<u8> {
    format!("durability matrix payload {i}\n")
        .repeat(64)
        .into_bytes()
}

/// The name the staging child gives file `i`. Zero-padded so a directory
/// listing in a failure message sorts the way the commits happened.
fn staged_name(i: usize) -> String {
    format!("staged-{i:04}.txt")
}

/// A multi-megabyte payload whose bytes vary, so a comparison catches a
/// transfer that dropped or reordered a block rather than only one that
/// truncated.
fn lfs_payload() -> Vec<u8> {
    (0..LFS_PAYLOAD_BYTES).map(|i| (i % 251) as u8).collect()
}

fn sha256_hex(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

fn run_git(cwd: &Path, args: &[&str]) {
    let status = Command::new("git")
        .args(args)
        .current_dir(cwd)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .expect("run git");
    assert!(status.success(), "git {args:?} must succeed");
}

fn git_stdout(cwd: &Path, args: &[&str]) -> Vec<u8> {
    let out = Command::new("git")
        .args(args)
        .current_dir(cwd)
        .output()
        .expect("run git");
    assert!(
        out.status.success(),
        "git {args:?} must succeed:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    out.stdout
}

fn git_text(cwd: &Path, args: &[&str]) -> String {
    String::from_utf8_lossy(&git_stdout(cwd, args)).into_owned()
}

/// A profile folder with a git repository in it, configured the way a real one
/// is. The identity is set because `git merge` and the fixture commits below go
/// through the `git` binary, which refuses to run without one.
fn init_profile_repo(work: &Path) {
    std::fs::create_dir_all(work).expect("create the profile folder");
    run_git(work, &["init", "-q", "-b", "main"]);
    run_git(work, &["config", "user.email", "durability@keeper.invalid"]);
    run_git(work, &["config", "user.name", "durability matrix"]);
}

/// Commit `added` through the production staging path.
///
/// Deliberately the real [`git::commit::stage_and_commit`] rather than
/// `git add && git commit`: the index-before-reference ordering this file
/// asserts (`commit.rs:267-272`) is keeper's own, and a fixture that used the
/// `git` binary would prove a property of git instead.
fn commit_paths(
    repo: &gix::Repository,
    work: &Path,
    added: &[&str],
) -> keeper_sync::Result<Option<gix::hash::ObjectId>> {
    let profile = SyncProfile::new(
        "01JDURABILITYMATRIX",
        "durability",
        work,
        "https://git.invalid/durability.git",
    );
    let provenance = Provenance::new(
        "durability",
        "matrix",
        "01JDURABILITYDEVICE",
        "matrix-host",
        SyncSource::Cli,
    );
    // A fixed authoring time keeps a commit id a function of its content, so a
    // failure message quoting one is reproducible.
    let author = gix::actor::Signature {
        name: "durability matrix".into(),
        email: "durability@keeper.invalid".into(),
        time: gix::date::Time::new(1_700_000_000, 0),
    };
    let changes = StagedChange {
        added: added.iter().map(|path| PathBuf::from(*path)).collect(),
        ..Default::default()
    };
    // No LFS routing: these cells are about the git boundaries, and a pointer
    // substitution would put a second subsystem inside the assertion.
    let substitutions: BTreeMap<PathBuf, Vec<u8>> = BTreeMap::new();
    git::commit::stage_and_commit(
        repo,
        &changes,
        &provenance,
        &profile,
        &author,
        &substitutions,
        None,
    )
}

// ---------------------------------------------------------------------------
// Shared assertions — half (b) of the AC
// ---------------------------------------------------------------------------

/// git's own integrity check, plus the cheapest thing that fails outright on an
/// unreadable index.
///
/// A half-written pack, a truncated loose object or a torn index all land here,
/// which is exactly the corruption NFR-24 forbids. "It did not panic" would
/// pass on every one of them.
fn assert_repository_sound(work: &Path, context: &str) {
    let fsck = Command::new("git")
        .args(["fsck", "--full", "--strict"])
        .current_dir(work)
        .output()
        .expect("run git fsck");
    assert!(
        fsck.status.success(),
        "{context}: the object store must not be corrupt:\n{}",
        String::from_utf8_lossy(&fsck.stderr)
    );

    let status = Command::new("git")
        .args(["status", "--porcelain"])
        .current_dir(work)
        .output()
        .expect("run git status");
    assert!(
        status.status.success(),
        "{context}: the index must stay readable:\n{}",
        String::from_utf8_lossy(&status.stderr)
    );
}

/// Not one byte of a file the user gave us may differ.
fn assert_file_bytes(work: &Path, name: &str, want: &[u8], context: &str) {
    let got = std::fs::read(work.join(name))
        .unwrap_or_else(|err| panic!("{context}: {name} must still exist: {err}"));
    assert_eq!(
        got.len(),
        want.len(),
        "{context}: {name} must not have been truncated or grown"
    );
    // `assert!` rather than `assert_eq!`: these buffers run to megabytes and a
    // failure message holding two of them is unreadable. The length is compared
    // separately above, so the diagnosis is not lost.
    assert!(got == want, "{context}: {name} must be byte-identical");
}

/// The same demand, made of the committed tree rather than the worktree: a
/// commit that recorded different bytes than the file held has lost the file
/// just as surely as a delete would.
fn assert_blob_bytes(work: &Path, revision: &str, want: &[u8], context: &str) {
    let got = git_stdout(work, &["cat-file", "blob", revision]);
    assert_eq!(
        got.len(),
        want.len(),
        "{context}: {revision} must record the whole file"
    );
    // See `assert_file_bytes` for why this is not `assert_eq!`.
    assert!(got == want, "{context}: {revision} must be byte-identical");
}

/// Stand in for the minute the index-lock recovery is entitled to take.
///
/// `git::repo::STALE_INDEX_LOCK` is sixty seconds because an `index.lock`
/// younger than that may belong to a `git` command the user is running by hand,
/// and taking it would corrupt a real index to fix a problem nobody has. That
/// policy is correct and is **not** weakened here — backdating the lock is the
/// passage of time the engine is allowed, nothing more. The engine still has to
/// notice the lock and clear it, which is the part under test.
///
/// Reference locks are deliberately never aged: their recovery is a watch
/// rather than an age and completes in about two seconds, so the cells below
/// exercise it exactly as a user's machine would.
fn age_index_lock(work: &Path) {
    let lock = work.join(".git").join("index.lock");
    let Ok(file) = std::fs::File::options().write(true).open(&lock) else {
        return;
    };
    let long_ago = std::time::SystemTime::now() - Duration::from_secs(600);
    let _ = file.set_times(std::fs::FileTimes::new().set_modified(long_ago));
}

// ---------------------------------------------------------------------------
// The staging child, and the harness that kills it
// ---------------------------------------------------------------------------

/// A child that is SIGKILLed and reaped when it goes out of scope — including
/// on an assertion unwind.
///
/// `Child` does not kill on drop, so without this a cell that fails between
/// spawning and killing would leave a process spinning on a shared runner for
/// as long as the runner lived. Every kill cell drops this guard *before* it
/// asserts anything, so the kill is part of the scenario rather than a
/// consequence of it, and the guard is the backstop for every other path out.
struct KilledChild(Child);

impl Drop for KilledChild {
    fn drop(&mut self) {
        // `Child::kill` is SIGKILL on Unix — a real, uncatchable OS kill, with
        // no `unsafe` and no new dependency. Lenient: a child that already died
        // on its own (the unplug cell kills the media out from under it) makes
        // this error, and that is not a failure of the scenario.
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// Re-invoke this test binary as a staging writer against `work`.
///
/// `--exact` pins the single child entry point so nothing else in the binary
/// runs, and `--nocapture` is what lets its `committed …` lines reach the pipe
/// at all. The reader runs on its own thread so the parent can apply a
/// wall-clock deadline instead of blocking forever on a wedged child.
fn spawn_staging_child(work: &Path) -> (KilledChild, mpsc::Receiver<(usize, String)>) {
    let exe = std::env::current_exe().expect("locate the current test executable");
    let mut child = Command::new(&exe)
        .args([
            "--exact",
            "durability_child_stages_until_killed",
            "--nocapture",
        ])
        .env(DURABILITY_CHILD_ENV, work)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn the staging child");

    let stdout = child.stdout.take().expect("the child's stdout is piped");
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines() {
            // The pipe closes when the kill lands; that is the normal end.
            let Ok(line) = line else { break };
            let Some(rest) = line.strip_prefix("committed ") else {
                continue;
            };
            let mut fields = rest.split_whitespace();
            let (Some(index), Some(oid)) = (fields.next(), fields.next()) else {
                continue;
            };
            let Ok(index) = index.parse::<usize>() else {
                continue;
            };
            if tx.send((index, oid.to_owned())).is_err() {
                break; // the parent stopped listening
            }
        }
    });

    (KilledChild(child), rx)
}

/// Collect commits the child reported as landed, up to `want`.
///
/// Only reported commits are ever asserted on. That is the whole "torn final
/// write" discipline: the commit the child was in the middle of may legitimately
/// be absent, and a commit it told us about may not.
fn collect_reported_commits(
    rx: &mpsc::Receiver<(usize, String)>,
    want: usize,
) -> Vec<(usize, String)> {
    let mut seen = Vec::new();
    while seen.len() < want {
        match rx.recv_timeout(CHILD_DEADLINE) {
            Ok(entry) => seen.push(entry),
            Err(_) => break, // deadline, or the child exited and closed the pipe
        }
    }
    seen
}

fn assert_child_did_real_work(reported: &[(usize, String)], context: &str) {
    assert!(
        reported.len() >= KILL_AFTER,
        "{context}: the child reported only {} commits before stalling or exiting; \
         expected at least {KILL_AFTER}. A short count means the writer never got \
         going — investigate the child, not durability.",
        reported.len()
    );
}

/// Both halves of the AC after a kill during staging, driven exactly as a fresh
/// run drives them.
///
/// (b) first: every commit the child reported is still an ancestor of `HEAD`
/// and still records the exact bytes, the worktree copies are byte-identical,
/// and the repository passes git's own integrity check. Then (a): a brand-new
/// change must commit, which is the difference between "recovered" and "reopens
/// but can never make progress again" — a folder in the second state has lost
/// the user's data too, just later.
fn assert_recovered_after_kill(work: &Path, reported: &[(usize, String)], context: &str) {
    // The recovery a fresh run performs: `open` releases an `index.lock` or a
    // reference lock the kill abandoned (`repo.rs:72-73`). Nothing else is done
    // by hand — if this were not enough, the profile would be stranded on a
    // user's machine too.
    age_index_lock(work);
    let repo = git::repo::open(work, false)
        .unwrap_or_else(|err| panic!("{context}: a killed profile must reopen: {err}"));

    assert_repository_sound(work, context);

    for (index, oid) in reported {
        let name = staged_name(*index);
        let want = payload(*index);
        // Reachability, not mere existence: a commit that survived as a
        // dangling object is a commit whose work was published nowhere.
        run_git(work, &["merge-base", "--is-ancestor", oid.as_str(), "HEAD"]);
        assert_blob_bytes(work, &format!("HEAD:{name}"), &want, context);
        assert_file_bytes(work, &name, &want, context);
    }

    // (a) the next run resumes cleanly.
    let resumed_name = "after-the-kill.txt";
    let resumed_bytes = b"work committed after the kill\n";
    std::fs::write(work.join(resumed_name), resumed_bytes).expect("write the resuming change");
    let resumed = commit_paths(&repo, work, &[resumed_name])
        .unwrap_or_else(|err| panic!("{context}: the next run must be able to commit: {err}"))
        .unwrap_or_else(|| panic!("{context}: the resuming change must produce a commit"));
    assert_blob_bytes(
        work,
        &format!("{resumed}:{resumed_name}"),
        resumed_bytes,
        context,
    );
    // Nothing is left staged: whatever the kill had put in the index was
    // carried into this commit rather than stranded there.
    assert_eq!(
        git_text(work, &["diff", "--cached", "--name-only"]).trim(),
        "",
        "{context}: recovery must leave nothing stuck in the index"
    );
    assert_repository_sound(work, &format!("{context}, after resuming"));
}

/// The staging writer. A no-op unless [`DURABILITY_CHILD_ENV`] is set, so a
/// normal run executes it as a trivial pass and never recurses.
///
/// When it is set, this writes a file and commits it through the production
/// staging path in a tight loop, announcing each landed commit on stdout, until
/// the parent kills it or the media it is writing to disappears.
#[test]
fn durability_child_stages_until_killed() {
    let Some(work) = std::env::var_os(DURABILITY_CHILD_ENV).map(PathBuf::from) else {
        return;
    };
    let repo = git::repo::open(&work, false).expect("child: open the profile repository");
    let stdout = std::io::stdout();
    let mut i = 0usize;
    loop {
        let name = staged_name(i);
        std::fs::write(work.join(&name), payload(i)).expect("child: write the staged file");
        let committed = commit_paths(&repo, &work, &[name.as_str()])
            .expect("child: stage and commit")
            .expect("child: a new file must produce a commit");
        // Printed only after the call returned, so the parent's list contains
        // nothing but commits that genuinely landed.
        let mut lock = stdout.lock();
        let _ = writeln!(lock, "committed {i} {committed}");
        let _ = lock.flush();
        i += 1;
    }
}

// ---------------------------------------------------------------------------
// Cell: stage × kill -9
// ---------------------------------------------------------------------------

/// A real SIGKILL lands while the worktree → index path is running.
///
/// This is the one boundary where the *timing* is the point, so it is a real OS
/// kill of a real process rather than a planted artifact: `index.write` is not
/// atomic from the outside, and only a signal the process cannot catch proves
/// that nothing on the cleanup path is load-bearing.
#[test]
fn a_kill_during_staging_loses_no_committed_file_and_leaves_a_usable_index() {
    let dir = tempfile::tempdir().expect("tempdir");
    let work = dir.path().join("work");
    init_profile_repo(&work);

    let (child, rx) = spawn_staging_child(&work);
    let reported = collect_reported_commits(&rx, KILL_AFTER);
    // The kill, with the next stage still in flight. Dropping the guard here
    // rather than at the end of the test makes the kill part of the scenario
    // and guarantees nothing survives an assertion failure below.
    drop(child);

    assert_child_did_real_work(&reported, "a kill during staging");
    assert_recovered_after_kill(&work, &reported, "a kill during staging");
}

// ---------------------------------------------------------------------------
// Cell: stage × volume disappears
// ---------------------------------------------------------------------------

/// The media is pulled out while a stage is in flight, then plugged back in.
///
/// Renaming the mount root away is as close as a test can get to an unplug
/// without a real device: every absolute path under it stops resolving while a
/// writer still holds the folder open, which is precisely what the writer
/// experiences. The marker goes with it, so [`volume::scan`] sees what it would
/// see on a yanked pendrive.
///
/// Two things are asserted that the plain kill cell cannot: that the absence
/// reads as [`VolumeStatus::Absent`] rather than as a tree full of deletions
/// (AD-48 — "an absent volume is never a deletion", the one answer that would
/// license deleting the user's files), and that the work committed before the
/// unplug is byte-for-byte intact after the re-attach.
#[test]
fn a_volume_pulled_during_staging_is_never_a_deletion_and_loses_no_committed_file() {
    let dir = tempfile::tempdir().expect("tempdir");
    let mount = dir.path().join("mount");
    let detached = dir.path().join("detached");
    let work = mount.join("work");
    init_profile_repo(&work);

    let marker = VolumeMarker::new("durability matrix pendrive", 1_700_000_000_000);
    VolumeMarker::write(&mount, &marker).expect("adopt the volume");
    assert!(
        matches!(
            volume::scan(&work, Some(marker.volume_id.as_str())).expect("scan an attached volume"),
            VolumeStatus::Present { .. }
        ),
        "the fixture must start with the media attached, or the unplug proves nothing"
    );

    let (child, rx) = spawn_staging_child(&work);
    let reported = collect_reported_commits(&rx, KILL_AFTER);
    // The unplug itself. The result is captured rather than unwrapped so the
    // child is still killed if the rename fails.
    let unplugged = std::fs::rename(&mount, &detached);
    drop(child);

    assert!(
        unplugged.is_ok(),
        "the fixture must be able to detach the volume: {unplugged:?}"
    );
    assert_child_did_real_work(&reported, "a volume pulled during staging");

    // While the media is gone, the profile's whole tree is missing — and that
    // must read as absence, not as the user having deleted everything.
    assert_eq!(
        volume::scan(&work, Some(marker.volume_id.as_str())).expect("scan a detached volume"),
        VolumeStatus::Absent,
        "a pulled volume must read as absent, never as a tree of deletions"
    );

    // Re-attach.
    std::fs::rename(&detached, &mount).expect("re-attach the volume");
    assert!(
        matches!(
            volume::scan(&work, Some(marker.volume_id.as_str()))
                .expect("scan a re-attached volume"),
            VolumeStatus::Present { marker: found } if found.volume_id == marker.volume_id
        ),
        "the re-attached volume must be recognised as the same one"
    );

    assert_recovered_after_kill(&work, &reported, "a volume pulled during staging");
}

// ---------------------------------------------------------------------------
// Cell: commit × kill -9
// ---------------------------------------------------------------------------

/// A commit dies between the index write and the reference transaction.
///
/// `stage_and_commit` writes the index (`commit.rs:270`) before it opens the
/// reference transaction (`commit.rs:319`) precisely so a crash in between
/// leaves staged-but-uncommitted work the next run re-drives, rather than
/// losing the staging altogether. This cell is that claim, made falsifiable.
///
/// The fault is a planted `HEAD.lock` rather than a raced signal, and that is a
/// strictly stronger fixture here: it reproduces the state a kill inside the
/// transaction leaves — the lock the CI failure on `release/v0.6.3` left
/// behind — on every run and every machine, where a race would land in the
/// window a few microseconds wide only sometimes. `HEAD` is the first reference
/// the transaction locks and therefore the one a kill most often strands. The
/// racing half lives in `keeper-syncd/tests/durability_matrix.rs`.
#[test]
fn a_commit_interrupted_between_the_index_and_the_reference_resumes_cleanly() {
    let dir = tempfile::tempdir().expect("tempdir");
    let work = dir.path().join("work");
    init_profile_repo(&work);

    // Opened before the lock is planted, so the open-time recovery does not
    // clear the fault before the cell has used it.
    let repo = git::repo::open(&work, false).expect("open the profile repository");
    let first_bytes = payload(0);
    std::fs::write(work.join("first.txt"), &first_bytes).expect("write the first file");
    let first = commit_paths(&repo, &work, &["first.txt"])
        .expect("the first commit must succeed")
        .expect("a new file must produce a commit");

    let interrupted_bytes = payload(1);
    std::fs::write(work.join("second.txt"), &interrupted_bytes).expect("write the second file");
    let lock = work.join(".git").join("HEAD.lock");
    // The content gitoxide had written into it: the id of a commit that exists
    // locally and has not been published.
    std::fs::write(&lock, format!("{first}\n")).expect("plant the reference lock");

    let interrupted = commit_paths(&repo, &work, &["second.txt"]);
    assert!(
        interrupted.is_err(),
        "an abandoned HEAD lock must stop the commit, or the fault was never injected"
    );

    // (b) no byte lost, no corrupt index.
    let context = "a commit interrupted at the reference transaction";
    assert_file_bytes(&work, "second.txt", &interrupted_bytes, context);
    assert_repository_sound(&work, context);
    assert_eq!(
        git_text(&work, &["rev-parse", "HEAD"]).trim(),
        first.to_string(),
        "{context}: HEAD must not have moved"
    );
    // The staging survived the failure — this is the whole reason the index is
    // written first, and the difference between a repeat and a loss.
    assert!(
        git_text(&work, &["diff", "--cached", "--name-only"])
            .lines()
            .any(|line| line == "second.txt"),
        "{context}: the staged path must survive for the next run to re-drive"
    );

    // (a) the next run resumes cleanly: reopening the repository is what a
    // fresh run does, and it releases the abandoned lock on its own.
    let repo = git::repo::open(&work, false).expect("a locked profile must reopen");
    assert!(
        !lock.exists(),
        "{context}: the abandoned lock must be gone, not merely worked around"
    );
    let resumed = commit_paths(&repo, &work, &["second.txt"])
        .expect("the retry must commit")
        .expect("the re-driven change must produce a commit");
    assert_ne!(resumed, first, "{context}: the retry must advance HEAD");
    assert_blob_bytes(&work, "HEAD:second.txt", &interrupted_bytes, context);
    assert_blob_bytes(&work, "HEAD:first.txt", &first_bytes, context);
    assert_eq!(
        git_text(&work, &["status", "--porcelain"]).trim(),
        "",
        "{context}: the resumed commit must leave a clean tree"
    );
    assert_repository_sound(&work, &format!("{context}, after resuming"));
}

// ---------------------------------------------------------------------------
// Cell: push × network loss
// ---------------------------------------------------------------------------

/// Accept every connection and drop it without answering.
///
/// The listener is moved onto its own thread and loops, so a retry meets the
/// same wall rather than a closed port — "the network went away" rather than
/// "nothing was ever listening". The thread ends when the process does.
fn accept_and_drop(listener: TcpListener) {
    std::thread::spawn(move || {
        while let Ok((stream, _)) = listener.accept() {
            let _ = stream.shutdown(std::net::Shutdown::Both);
        }
    });
}

/// A push loses the connection, and the journal is what makes that a repeat
/// rather than a loss.
///
/// The remote is `git://127.0.0.1:<ephemeral>` on a listener that accepts and
/// hangs up. `git://` is deliberate over `http://`: it is a bare TCP
/// conversation, so the fault is unambiguously a dropped connection and no
/// proxy configuration on the runner can get between the two ends. The port is
/// whatever the kernel hands out — nothing here is fixed.
///
/// Both halves are asserted where each one lives. (b) is local: the commit, the
/// worktree and the index are exactly as they were, because a failed push must
/// touch none of them. (a) is the journal's: the unit was written before the
/// attempt and is still there afterwards, so the supervisor re-offers it, and
/// the retry against a reachable remote publishes bytes identical to the
/// worktree's. "Resumes cleanly" is the journal replaying to a consistent
/// state, not the first attempt somehow finishing.
#[test]
fn a_push_cut_off_by_network_loss_keeps_its_journal_unit_and_publishes_on_the_retry() {
    let dir = tempfile::tempdir().expect("tempdir");
    let work = dir.path().join("work");
    init_profile_repo(&work);

    let published = payload(7);
    std::fs::write(work.join("note.txt"), &published).expect("write the file to publish");
    let repo = git::repo::open(&work, false).expect("open the profile repository");
    commit_paths(&repo, &work, &["note.txt"])
        .expect("the commit must succeed")
        .expect("a new file must produce a commit");
    let head_before = git_text(&work, &["rev-parse", "HEAD"]).trim().to_owned();

    // The journal unit, recorded before the attempt exactly as the engine does.
    let conn = db::open_in_memory().expect("open the journal");
    let unit = db::enqueue(&conn, "01JDURABILITYMATRIX", &WorkKind::Push, 0, 0)
        .expect("journal the push before attempting it");
    let claimed = db::claim_ready(&conn, "01JDURABILITYMATRIX", 0, 8).expect("claim the push");
    assert_eq!(claimed.len(), 1, "the queued push must be claimable");

    let dead = TcpListener::bind("127.0.0.1:0").expect("bind an ephemeral port");
    let dead_port = dead.local_addr().expect("read the bound port").port();
    accept_and_drop(dead);

    let cli = GitCli::new(PathBuf::from("git"));
    let cut_off = cli.push(
        &work,
        &format!("git://127.0.0.1:{dead_port}/durability.git"),
        "refs/heads/main:refs/heads/main",
        None,
    );
    assert!(
        cut_off.is_err(),
        "a remote that hangs up must fail the push, or the fault was never injected"
    );

    // (b) nothing local moved and nothing local is corrupt.
    let context = "a push cut off by network loss";
    assert_file_bytes(&work, "note.txt", &published, context);
    assert_blob_bytes(&work, "HEAD:note.txt", &published, context);
    assert_eq!(
        git_text(&work, &["rev-parse", "HEAD"]).trim(),
        head_before,
        "{context}: a failed push must not move HEAD"
    );
    assert_eq!(
        git_text(&work, &["status", "--porcelain"]).trim(),
        "",
        "{context}: a failed push must not disturb the index"
    );
    assert_repository_sound(&work, context);

    // (a) the unit is still owed. The supervisor's own failure path returns it
    // to the queue; what matters is that it is re-offered rather than dropped.
    db::reschedule(
        &conn,
        unit,
        WorkState::Pending,
        0,
        Some("the remote hung up"),
    )
    .expect("return the failed unit to the queue");
    let reoffered = db::claim_ready(&conn, "01JDURABILITYMATRIX", 0, 8).expect("re-claim");
    assert_eq!(
        reoffered.len(),
        1,
        "{context}: the push must still be owed after the failure"
    );
    assert_eq!(
        reoffered[0].id, unit,
        "{context}: the re-offered unit must be the same one, not a fresh guess"
    );
    assert_eq!(
        reoffered[0].attempts, 2,
        "{context}: the retry must be counted, or backoff has nothing to grow from"
    );

    // The retry, against a remote that is there.
    let remote = dir.path().join("remote.git");
    run_git(
        dir.path(),
        &["init", "--bare", "-q", "-b", "main", "remote.git"],
    );
    let remote_url = remote.to_str().expect("utf-8 remote path");
    cli.push(&work, remote_url, "refs/heads/main:refs/heads/main", None)
        .expect("the retry must publish");
    db::complete(&conn, unit).expect("clear the delivered unit");
    assert!(
        db::claim_ready(&conn, "01JDURABILITYMATRIX", 0, 8)
            .expect("claim after completion")
            .is_empty(),
        "{context}: a delivered push must leave the journal empty"
    );

    // Not one byte lost between the worktree and the remote.
    assert_blob_bytes(&remote, "refs/heads/main:note.txt", &published, context);
    assert_eq!(
        git_text(&remote, &["rev-parse", "refs/heads/main"]).trim(),
        head_before,
        "{context}: the remote must hold exactly the commit that was owed"
    );
}

// ---------------------------------------------------------------------------
// Cell: LFS upload × network loss
// ---------------------------------------------------------------------------

/// Read a little of the request, then hang up.
///
/// Reading first is what makes this a *mid-body* loss rather than a refused
/// connection: the client has already handed bytes over when the peer
/// disappears, which is the fault [`LFS_PAYLOAD_BYTES`] is sized to guarantee.
fn accept_read_a_little_then_drop(listener: TcpListener) {
    std::thread::spawn(move || {
        while let Ok((mut stream, _)) = listener.accept() {
            // Sink a couple of reads so the peer disappears with the client's
            // bytes already in flight. The amount is matched rather than
            // discarded: `let _ = read(..)` is exactly the ignored-io-amount
            // mistake, and here the count is also the evidence that anything
            // was in flight at all.
            let mut scratch = [0u8; 4096];
            let mut sunk = 0usize;
            for _ in 0..2 {
                match stream.read(&mut scratch) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => sunk += n,
                }
            }
            // Reported rather than asserted: this runs on a helper thread,
            // where a panic would be noise instead of a failure. A zero shows
            // up as the cell's own failure anyway — an upload that lost the
            // connection before sending anything was not cut off mid body.
            if sunk == 0 {
                eprintln!("durability matrix: the LFS peer hung up before any bytes arrived");
            }
            let _ = stream.shutdown(std::net::Shutdown::Both);
        }
    });
}

/// Serve exactly one `PUT`, answer 200, and hand the body back over `sink`.
///
/// Minimal on purpose: the LFS `basic` adapter sends one request with an exact
/// `Content-Length` (`basic.rs:664`), so reading the head to that header and
/// then exactly that many bytes is the whole protocol this cell needs. The body
/// is forwarded rather than checksummed so a failure can say *how* it differed.
fn serve_one_put(listener: TcpListener, sink: mpsc::Sender<Vec<u8>>) {
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
        let _ = stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n");
        let _ = stream.flush();
        let _ = sink.send(body);
    });
}

/// The connection dies with the object half sent.
///
/// Uploads are not resumable — upstream's own adapter says so, and
/// `basic.rs:634` says it again — so "resumes cleanly" here means the *next*
/// attempt sends the object from zero and every byte arrives. The failure must
/// therefore leave nothing behind that a later run could mistake for the
/// object: a correctly named file holding a prefix would be handed to the
/// smudge filter as if it were the content (`store.rs:100-104`).
///
/// Both halves: (b) the source is untouched, the store holds neither a finished
/// nor a staged partial object, and the bytes that finally reach the server are
/// compared one for one against the source; (a) the retry against a reachable
/// endpoint succeeds. The client is built `no_proxy` so an ambient proxy
/// variable on the runner cannot silently redirect either endpoint.
#[tokio::test]
async fn an_lfs_upload_cut_off_mid_body_stores_no_partial_object_and_re_sends_every_byte() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = LfsStore::in_git_dir(dir.path().join(".git"));
    store.ensure_layout().expect("lay out the object store");

    let bytes = lfs_payload();
    let oid = sha256_hex(&bytes);
    let source = dir.path().join("source.bin");
    std::fs::write(&source, &bytes).expect("write the upload source");

    let dead = TcpListener::bind("127.0.0.1:0").expect("bind an ephemeral port");
    let dead_port = dead.local_addr().expect("read the bound port").port();
    accept_read_a_little_then_drop(dead);

    let client = reqwest::Client::builder()
        .no_proxy()
        .build()
        .expect("build the transfer client");
    let transfer = BasicTransfer::new(client, store.clone());

    let cut_off = transfer
        .upload(
            &upload_spec(&oid, bytes.len() as u64, dead_port),
            &source,
            None,
        )
        .await;
    let context = "an LFS upload cut off mid body";
    match cut_off {
        Err(SyncError::Network { .. }) => {}
        other => panic!(
            "{context}: a dropped connection must surface as a network failure, got {other:?}"
        ),
    }

    // (b) nothing lost locally, and nothing left that could be mistaken for the
    // object later.
    assert_file_bytes(dir.path(), "source.bin", &bytes, context);
    assert!(
        !store.contains(&oid, bytes.len() as u64),
        "{context}: a failed upload must not mark the object as stored"
    );
    assert!(
        !store.object_path(&oid).exists(),
        "{context}: a failed upload must leave no object file at all"
    );
    assert!(
        !store.incomplete_path(&oid).exists(),
        "{context}: a failed upload must leave no staged prefix behind"
    );

    // (a) the retry sends the whole object.
    let live = TcpListener::bind("127.0.0.1:0").expect("bind an ephemeral port");
    let live_port = live.local_addr().expect("read the bound port").port();
    let (tx, rx) = mpsc::channel();
    serve_one_put(live, tx);

    transfer
        .upload(
            &upload_spec(&oid, bytes.len() as u64, live_port),
            &source,
            None,
        )
        .await
        .expect("the retry must upload");

    let received = rx
        .recv_timeout(CHILD_DEADLINE)
        .expect("the stub server must report the body it received");
    assert_eq!(
        received.len(),
        bytes.len(),
        "{context}: the retry must send the whole object"
    );
    assert!(
        received == bytes,
        "{context}: every byte the server received must match the source"
    );
    assert_eq!(
        sha256_hex(&received),
        oid,
        "{context}: the object that arrived must hash to the oid that was owed"
    );
}

/// A batch response offering one `PUT` at `port`, with no `verify` callback —
/// the shape Forgejo returns for an object the server does not yet hold.
fn upload_spec(oid: &str, size: u64, port: u16) -> ObjectSpec {
    ObjectSpec {
        oid: oid.to_owned(),
        size,
        actions: Some(Actions {
            upload: Some(Action {
                href: format!("http://127.0.0.1:{port}/objects/{oid}"),
                ..Default::default()
            }),
            ..Default::default()
        }),
        ..Default::default()
    }
}

// ---------------------------------------------------------------------------
// Cell: checkout × kill -9
// ---------------------------------------------------------------------------

/// A checkout dies while it holds the index, and the remote change still lands.
///
/// Applying a fetched commit is the fifth operation that has to shell out
/// (`cli.rs:184-192`), and `git merge --ff-only` holds `index.lock` for the
/// whole index-and-worktree update. So the artifact a kill leaves there is that
/// lock — planted here rather than raced for, so the cell reproduces on every
/// run — and the failure mode it causes is the one NFR-24 singles out: git then
/// refuses every subsequent index write, the profile never syncs again, and the
/// cure is a human deleting a file they have no reason to know exists
/// (`repo.rs:77-83`).
///
/// (b) is asserted at the moment of the failure: the worktree still holds the
/// *old* bytes exactly, so nothing was half-applied, and the repository is
/// sound. (a) is asserted after the recovery the engine actually performs —
/// reopening the repository — with the incoming content compared byte for byte
/// and the branch required to have reached the remote's tip.
#[test]
fn a_checkout_interrupted_while_it_held_the_index_applies_the_remote_change_on_the_retry() {
    let dir = tempfile::tempdir().expect("tempdir");
    run_git(
        dir.path(),
        &["init", "--bare", "-q", "-b", "main", "remote.git"],
    );
    let remote = dir.path().join("remote.git");
    let remote_url = remote.to_str().expect("utf-8 remote path");

    // A peer publishes v1, we clone it, then the peer publishes v2. Fixture
    // work, so it goes through the `git` binary; the code under test is the
    // checkout below.
    let seed = dir.path().join("seed");
    init_profile_repo(&seed);
    let v1 = payload(11);
    let v2 = payload(12);
    std::fs::write(seed.join("shared.txt"), &v1).expect("write v1");
    run_git(&seed, &["add", "shared.txt"]);
    run_git(&seed, &["commit", "-q", "-m", "v1"]);
    run_git(&seed, &["push", "-q", remote_url, "main:main"]);

    run_git(dir.path(), &["clone", "-q", remote_url, "work"]);
    let work = dir.path().join("work");

    std::fs::write(seed.join("shared.txt"), &v2).expect("write v2");
    run_git(&seed, &["add", "shared.txt"]);
    run_git(&seed, &["commit", "-q", "-m", "v2"]);
    run_git(&seed, &["push", "-q", remote_url, "main:main"]);
    run_git(&work, &["fetch", "-q", "origin"]);

    // The artifact the kill leaves.
    let lock = work.join(".git").join("index.lock");
    std::fs::write(&lock, b"").expect("plant the index lock");

    let cli = GitCli::new(PathBuf::from("git"));
    let interrupted = cli.merge_ff_only(&work, "origin/main");
    assert!(
        interrupted.is_err(),
        "an abandoned index lock must stop the checkout, or the fault was never injected"
    );

    // (b) nothing half-applied, nothing corrupt.
    let context = "a checkout interrupted while it held the index";
    assert_file_bytes(&work, "shared.txt", &v1, context);
    assert_repository_sound(&work, context);

    // (a) the recovery a fresh run performs, and then the checkout must land.
    age_index_lock(&work);
    git::repo::open(&work, false).expect("a locked profile must reopen");
    assert!(
        !lock.exists(),
        "{context}: the abandoned lock must be gone, not merely worked around"
    );
    cli.merge_ff_only(&work, "origin/main")
        .expect("the retry must apply the remote change");

    assert_file_bytes(&work, "shared.txt", &v2, context);
    assert_blob_bytes(&work, "HEAD:shared.txt", &v2, context);
    assert_eq!(
        git_text(&work, &["rev-parse", "HEAD"]).trim(),
        git_text(&work, &["rev-parse", "origin/main"]).trim(),
        "{context}: the retry must reach the remote's tip"
    );
    assert_eq!(
        git_text(&work, &["status", "--porcelain"]).trim(),
        "",
        "{context}: the applied checkout must leave a clean tree"
    );
    assert_repository_sound(&work, &format!("{context}, after resuming"));
}
