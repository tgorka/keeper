//! NFR-24: a `SIGKILL` at any instant costs no data and corrupts no index.
//!
//! The interesting failures are not the ones a unit test can stage. They come
//! from the process dying while git holds `index.lock`, while a pack is half
//! written, or between a commit and the push that publishes it — states no
//! in-process fault injection reproduces faithfully, because the whole point is
//! that nothing gets to run a cleanup path.
//!
//! So this spawns the real daemon and kills it with the one signal it cannot
//! catch, at a dense sweep of instants spanning an entire sync. Rather than
//! claiming to have hit each named stage — which would be a timing guess
//! dressed up as coverage — every iteration records what the kill actually
//! interrupted, read off the artifacts left behind, and the run asserts two
//! things at once: the invariants hold at every instant, and the sweep as a
//! whole genuinely reached the stages this is supposed to cover.
//!
//! The recovery assertion is deliberately end-to-end. "The index is readable"
//! is necessary but far too weak: a daemon that recovers into a repository it
//! can never push again has still lost the user's data, just later.

use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// What a kill was observed to have interrupted.
///
/// Inferred from artifacts rather than from a progress event, because a
/// progress event is a claim by the process about itself and this test exists
/// precisely for the case where that process died mid-claim.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Interrupted {
    /// Killed before the working tree was scanned into the index.
    BeforeStaging,
    /// Killed with git's index lock still on disk.
    MidIndexWrite,
    /// Committed locally, nothing published yet.
    AfterCommitBeforePush,
    /// The remote already has the commit; the kill landed on the tail.
    AfterPush,
}

struct Peer {
    root: PathBuf,
    work: PathBuf,
    remote: PathBuf,
}

impl Peer {
    /// A profile whose timings are compressed so a sweep is affordable.
    ///
    /// The settle window is the only thing shortened, and it is a gate on
    /// *observation*, not on any durability property — a file still has to be
    /// seen twice, just sooner.
    fn new(label: &str, files: usize, big_file_bytes: usize) -> Self {
        let root = std::env::temp_dir().join(format!(
            "keeper-durability-{label}-{}-{}",
            std::process::id(),
            now_nanos()
        ));
        let work = root.join("work");
        let remote = root.join("remote.git");
        for dir in [
            &work,
            &root.join("cfg"),
            &root.join("data"),
            &root.join("state"),
        ] {
            std::fs::create_dir_all(dir).expect("create peer directories");
        }
        run_git(
            &root,
            &["init", "--bare", "-q", "-b", "main", remote_str(&remote)],
        );

        for i in 0..files {
            std::fs::write(work.join(format!("file-{i:04}.txt")), payload(i))
                .expect("seed the working tree");
        }
        if big_file_bytes > 0 {
            // Above the LFS threshold set below, so the sweep also crosses the
            // large-object path rather than only the plain-blob one.
            std::fs::write(work.join("big.bin"), vec![b'k'; big_file_bytes])
                .expect("seed the large file");
        }

        let peer = Self { root, work, remote };
        peer.daemon(&["init"]).status().expect("init the config");
        let status = peer
            .daemon(&[
                "add",
                "--name",
                "dur",
                "--path",
                peer.work.to_str().expect("utf-8 work path"),
                "--remote",
                peer.remote.to_str().expect("utf-8 remote path"),
                "--branch",
                "main",
                "--settle-ms",
                "0",
                "--lfs-threshold-bytes",
                "1048576",
            ])
            .status()
            .expect("register the profile");
        assert!(status.success(), "the profile must register");
        peer
    }

    fn daemon(&self, args: &[&str]) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_keeper-syncd"));
        command
            .args(args)
            .env("XDG_CONFIG_HOME", self.root.join("cfg"))
            .env("XDG_DATA_HOME", self.root.join("data"))
            .env("XDG_STATE_HOME", self.root.join("state"))
            // A committer identity must not leak in from the developer's own
            // git config: the daemon is expected to supply its own, and a test
            // that borrowed the host's would hide that.
            .env_remove("GIT_AUTHOR_NAME")
            .env_remove("GIT_COMMITTER_NAME")
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        command
    }

    /// Start a sync, kill it after `delay`, and report what it interrupted.
    fn kill_sync_after(&self, delay: Duration) -> Interrupted {
        let mut child = self
            .daemon(&["sync", "--once"])
            .spawn()
            .expect("spawn a sync");
        std::thread::sleep(delay);
        // SIGKILL: the process gets no chance to finalize, which is the whole
        // point. A graceful stop is a different test.
        let _ = child.kill();
        let _ = child.wait();
        self.observe()
    }

    fn observe(&self) -> Interrupted {
        if self.work.join(".git/index.lock").exists() {
            return Interrupted::MidIndexWrite;
        }
        let local = self.rev_parse(&self.work, "HEAD");
        let Some(local) = local else {
            return Interrupted::BeforeStaging;
        };
        match self.rev_parse(&self.remote, "main") {
            Some(remote) if remote == local => Interrupted::AfterPush,
            _ => Interrupted::AfterCommitBeforePush,
        }
    }

    fn rev_parse(&self, repo: &Path, reference: &str) -> Option<String> {
        let out = Command::new("git")
            .args(["rev-parse", "--verify", "--quiet", reference])
            .current_dir(repo)
            .output()
            .ok()?;
        let text = String::from_utf8_lossy(&out.stdout).trim().to_owned();
        (out.status.success() && !text.is_empty()).then_some(text)
    }

    /// Every invariant that must hold after a kill, before any recovery runs.
    ///
    /// Checked *before* syncing again on purpose: a repository that only looks
    /// healthy after the daemon has repaired it would pass a weaker test while
    /// still being unusable to the human whose folder it is.
    fn assert_intact(&self, expected: &[(String, Vec<u8>)], at: Duration) {
        let context = format!("after a kill at {at:?}");

        // A kill early enough that the repository was never *finished* leaves
        // nothing for git to check. The user's own files still must be
        // untouched, which is asserted below either way — but running fsck
        // against a plain directory would fail the test for the wrong reason.
        //
        // Keyed on `HEAD` rather than on `.git` itself: `gix::init` lays down
        // `info/` and `hooks/` before it writes `HEAD`, so a kill inside that
        // window leaves a `.git` that exists and is not a repository — no
        // objects, no index, nothing that can be corrupt, and `git fsck`
        // reports "not a git repository". Which side of that window a kill
        // lands on is a property of the machine and of how much work the
        // daemon does before it, so testing for the directory alone made this
        // sweep pass or fail on timing rather than on durability.
        if !self.work.join(".git").join("HEAD").exists() {
            assert_files_intact(&self.work, expected, &context);
            return;
        }

        // git's own integrity check. A half-written pack or a truncated object
        // fails here, which is exactly the corruption NFR-24 forbids.
        let fsck = Command::new("git")
            .args(["fsck", "--full", "--strict"])
            .current_dir(&self.work)
            .output()
            .expect("run git fsck");
        assert!(
            fsck.status.success(),
            "{context}: the object store must not be corrupt:\n{}",
            String::from_utf8_lossy(&fsck.stderr)
        );

        // A readable index. `git status` is the cheapest thing that fails
        // outright on a corrupt one.
        let status = Command::new("git")
            .args(["status", "--porcelain"])
            .current_dir(&self.work)
            .output()
            .expect("run git status");
        assert!(
            status.status.success(),
            "{context}: the index must stay readable:\n{}",
            String::from_utf8_lossy(&status.stderr)
        );

        assert_files_intact(&self.work, expected, &context);
    }

    /// Stand in for the minute the index-lock recovery is entitled to take.
    ///
    /// `git::repo::STALE_INDEX_LOCK` is deliberately sixty seconds: an
    /// `index.lock` younger than that may still belong to a `git` command the
    /// user is running by hand, and taking it would corrupt a real index to
    /// fix a problem nobody has. That policy is correct and is not weakened —
    /// not in the engine, and not by shortening it here.
    ///
    /// But the passes below run inside a single second, so a kill that leaves
    /// an `index.lock` used to make this helper assert a convergence deadline
    /// the daemon never promised: the profile *does* recover, sixty seconds
    /// later, and the test failed roughly one run in thirty on that timing
    /// accident rather than on any defect. Backdating the lock is the passage
    /// of time the daemon is allowed, nothing more. The daemon still has to
    /// notice the lock and clear it, which is the part under test.
    ///
    /// Reference locks are deliberately NOT aged. Their recovery is a watch
    /// rather than an age and completes in seconds, so the sweep exercises it
    /// exactly as a user's machine would.
    fn age_any_index_lock(&self) {
        let lock = self.work.join(".git").join("index.lock");
        let Ok(file) = std::fs::File::options().write(true).open(&lock) else {
            return;
        };
        let long_ago = std::time::SystemTime::now() - Duration::from_secs(600);
        let _ = file.set_times(std::fs::FileTimes::new().set_modified(long_ago));
    }

    /// Drive the profile to a settled state the way the daemon would, keeping
    /// what each pass said.
    ///
    /// The output matters because this helper feeds a convergence assertion. It
    /// used to discard the exit status, so "the daemon converged" and "every
    /// pass failed" were indistinguishable and a failure read only as
    /// `left: 0, right: 12` — true, useless, and impossible to act on from a CI
    /// log. Returning the last failing pass's stderr means the next failure
    /// names its own cause (a stale `index.lock` after a kill -9, say).
    fn sync_to_completion(&self) -> String {
        // The lock a kill may have left is aged once, before any pass runs:
        // see `age_any_index_lock` for why this is the passage of time rather
        // than a weakened rule.
        self.age_any_index_lock();
        let mut last_failure = String::new();
        for pass in 0..6 {
            // `daemon()` nulls both streams for the spawn-and-kill path; undo that
            // here or `output()` faithfully captures the nothing it was told to
            // produce, which is what made the first version of this diagnostic
            // report an empty reason.
            let out = self
                .daemon(&["sync", "--once"])
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .output();
            match out {
                Ok(out) if !out.status.success() => {
                    // Both streams: the daemon reports a failure through its
                    // tracing subscriber (stderr) AND its printer (stdout), and
                    // which one carries the text depends on the mode, so a
                    // helper that reads only one can still come back empty.
                    last_failure = format!(
                        "pass {pass} exited {}: stderr={:?} stdout={:?}",
                        out.status,
                        String::from_utf8_lossy(&out.stderr).trim(),
                        String::from_utf8_lossy(&out.stdout).trim()
                    );
                }
                Ok(_) => {}
                Err(err) => last_failure = format!("pass {pass} could not run: {err}"),
            }
        }
        last_failure
    }

    /// One sync pass, with both streams captured.
    ///
    /// The output is the point: a recovery that repairs somebody's `.git`
    /// without saying so is not acceptable, so the tests below read what the
    /// pass reported as well as what it left on disk.
    fn sync_once(&self) -> Output {
        self.daemon(&["sync", "--once"])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .expect("run a sync pass")
    }

    fn remote_files(&self) -> Vec<String> {
        let out = Command::new("git")
            .args(["ls-tree", "-r", "--name-only", "main"])
            .current_dir(&self.remote)
            .output()
            .expect("list the remote tree");
        String::from_utf8_lossy(&out.stdout)
            .lines()
            .map(str::to_owned)
            .collect()
    }
}

impl Drop for Peer {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

/// Not one byte of the user's own files may be lost.
///
/// The daemon is allowed to have committed them, or not to have reached them
/// yet; it is never allowed to have damaged them.
fn assert_files_intact(work: &Path, expected: &[(String, Vec<u8>)], context: &str) {
    for (name, want) in expected {
        let got = std::fs::read(work.join(name))
            .unwrap_or_else(|err| panic!("{context}: {name} must still exist: {err}"));
        assert_eq!(&got, want, "{context}: {name} must be byte-identical");
    }
}

fn payload(i: usize) -> Vec<u8> {
    format!("content of file {i}\n").repeat(64).into_bytes()
}

fn now_nanos() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0)
}

fn remote_str(path: &Path) -> &str {
    path.to_str().expect("utf-8 remote path")
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

fn git_available() -> bool {
    Command::new("git")
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

#[test]
fn a_kill_at_any_instant_costs_no_data_and_corrupts_no_index() {
    if !git_available() {
        return;
    }

    // A sweep rather than a single timing: the delays walk from "before the
    // process has opened anything" through "well past the push", so the run
    // covers the stage boundaries instead of betting on one of them.
    let delays = [0, 15, 30, 45, 60, 80, 110, 150, 200, 300, 450, 700];
    let mut reached = std::collections::BTreeSet::new();

    for ms in delays {
        let peer = Peer::new(&format!("sweep{ms}"), 40, 0);
        let expected: Vec<(String, Vec<u8>)> = (0..40)
            .map(|i| (format!("file-{i:04}.txt"), payload(i)))
            .collect();

        let at = Duration::from_millis(ms);
        reached.insert(peer.kill_sync_after(at));
        peer.assert_intact(&expected, at);

        // Recovery is the other half of the contract: "did not corrupt" is
        // worth nothing if the profile can never make progress again.
        let trouble = peer.sync_to_completion();
        let published = peer.remote_files();
        assert_eq!(
            published.len(),
            40,
            "after a kill at {at:?} the profile must still converge; published: {}; \
             last failing pass: {}",
            published.len(),
            if trouble.is_empty() { "none" } else { &trouble }
        );
    }

    // The sweep is only evidence if it actually landed in the states it claims
    // to cover. Without this the test could pass by killing after every sync
    // had already finished, asserting nothing.
    assert!(
        reached.contains(&Interrupted::BeforeStaging),
        "the sweep must include a kill before anything was staged; reached {reached:?}"
    );
    assert!(
        reached.len() >= 2,
        "the sweep must interrupt more than one stage; reached {reached:?}"
    );
}

#[test]
fn a_kill_during_a_large_object_transfer_leaves_the_object_recoverable() {
    if !git_available() {
        return;
    }

    // Large enough to cross the LFS threshold and to still be in flight when
    // the kill lands, so the interrupted work is the large-object path rather
    // than an ordinary blob.
    const BIG: usize = 24 * 1024 * 1024;
    let peer = Peer::new("lfs", 4, BIG);
    let mut expected: Vec<(String, Vec<u8>)> = (0..4)
        .map(|i| (format!("file-{i:04}.txt"), payload(i)))
        .collect();
    expected.push(("big.bin".to_owned(), vec![b'k'; BIG]));

    let at = Duration::from_millis(120);
    peer.kill_sync_after(at);
    peer.assert_intact(&expected, at);

    let trouble = peer.sync_to_completion();
    let published = peer.remote_files();
    assert!(
        published.iter().any(|p| p == "big.bin"),
        "the large object must still reach the remote after a mid-transfer kill; \
         published: {published:?}; last failing pass: {}",
        if trouble.is_empty() { "none" } else { &trouble }
    );

    // `git ls-tree` above proves only that a BLOB for the path is on the remote,
    // and for an LFS path that blob is a ~130-byte pointer. It was therefore
    // satisfied for as long as this test has existed by a remote that held a
    // pointer to content nobody but this machine had — the exact silent failure
    // Story 34.15 gates against. So the content itself is checked, in the store
    // a peer cloning this remote would read it from.
    let objects = peer.remote.join("lfs").join("objects");
    let mut found = Vec::new();
    let mut stack = vec![objects.clone()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if let Ok(meta) = entry.metadata() {
                found.push((path, meta.len()));
            }
        }
    }
    assert!(
        found.iter().any(|(_, len)| *len == BIG as u64),
        "the remote must hold the {BIG}-byte object its pointer names, not just \
         the pointer; {} held: {found:?}",
        objects.display()
    );
}

#[test]
fn a_kill_leaves_no_committed_work_unpublished_forever() {
    if !git_available() {
        return;
    }

    // The specific loss NFR-24 is about: work that was committed locally and
    // then orphaned because the process died before publishing it. Recovery
    // must pick it up without the user knowing anything happened.
    let peer = Peer::new("orphan", 12, 0);
    let expected: Vec<(String, Vec<u8>)> = (0..12)
        .map(|i| (format!("file-{i:04}.txt"), payload(i)))
        .collect();

    let mut observed_orphan = false;
    for ms in [40, 55, 70, 90, 120, 160] {
        let at = Duration::from_millis(ms);
        if peer.kill_sync_after(at) == Interrupted::AfterCommitBeforePush {
            observed_orphan = true;
            peer.assert_intact(&expected, at);
            break;
        }
    }

    let trouble = peer.sync_to_completion();
    assert_eq!(
        peer.remote_files().len(),
        12,
        "everything committed locally must end up published; last failing pass: {}",
        if trouble.is_empty() { "none" } else { &trouble }
    );
    // Reported rather than asserted: whether a kill lands in that exact window
    // is a race, and a test that failed because it did not is a flaky test, not
    // a broken daemon. The convergence assertion above holds either way.
    if !observed_orphan {
        eprintln!("note: no kill landed between commit and push in this run");
    }
}

/// The reference lock `commit_as` leaves when a kill lands inside its
/// transaction, planted rather than raced for.
///
/// `HEAD` is the first reference the transaction locks and therefore the one a
/// kill is most likely to strand — it is the lock the CI failure on
/// `release/v0.6.3` left behind. The content is what gitoxide had written into
/// it: the id of a commit that exists locally and was never published.
fn plant_head_lock(work: &Path, oid: &str) -> PathBuf {
    let lock = work.join(".git").join("HEAD.lock");
    std::fs::write(&lock, format!("{oid}\n")).expect("plant the reference lock");
    lock
}

#[test]
fn a_reference_lock_left_by_a_kill_does_not_strand_a_folder() {
    if !git_available() {
        return;
    }

    // The deterministic half of `a_kill_leaves_no_committed_work_unpublished_forever`.
    // That test has to race a SIGKILL into a window a few microseconds wide, so
    // a green run of it proves very little; this one plants exactly what the
    // kill leaves and asserts the folder digs itself out.
    let peer = Peer::new("reflock", 6, 0);
    assert!(
        peer.sync_once().status.success(),
        "the first pass must publish, or the fixture proves nothing"
    );
    let head = peer
        .rev_parse(&peer.work, "HEAD")
        .expect("the first pass committed");

    std::fs::write(
        peer.work.join("after-the-kill.txt"),
        b"work that must survive",
    )
    .expect("seed the change the killed run was publishing");
    let lock = plant_head_lock(&peer.work, &head);

    let out = peer.sync_once();
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    assert!(
        out.status.success(),
        "a folder holding an abandoned reference lock must recover on its own; \
         stderr={stderr}"
    );
    assert!(
        !lock.exists(),
        "the abandoned lock must be gone, not merely worked around"
    );
    assert!(
        peer.remote_files()
            .iter()
            .any(|path| path == "after-the-kill.txt"),
        "the work the kill interrupted must reach the remote; published: {:?}",
        peer.remote_files()
    );
    // Silently repairing someone's git directory is not acceptable, so the
    // repair has to be legible in the log rather than inferred from the fact
    // that syncing started working again.
    assert!(
        stderr.contains("released a reference lock left behind by a killed run"),
        "the recovery must say what it removed; stderr={stderr}"
    );
}

#[test]
fn a_reference_lock_a_live_writer_holds_is_left_alone() {
    if !git_available() {
        return;
    }

    // The direction that stops this from being "delete every `.lock`". These
    // folders are meant to be used with plain `git`, so the holder of a
    // reference lock is quite likely a person's own commit in progress, and
    // taking it would break their write to fix a problem nobody has.
    let peer = Peer::new("livelock", 6, 0);
    assert!(
        peer.sync_once().status.success(),
        "the first pass must publish, or the fixture proves nothing"
    );
    let head = peer
        .rev_parse(&peer.work, "HEAD")
        .expect("the first pass committed");
    let published = peer.remote_files();

    std::fs::write(
        peer.work.join("not-yours.txt"),
        b"must not be published here",
    )
    .expect("seed a change so the pass reaches the commit path");
    let lock = plant_head_lock(&peer.work, &head);

    // A writer that is actually working touches its lock. Keeping it moving
    // for as long as the pass runs is what a concurrent `git commit` looks
    // like from keeper's side.
    let writing = Arc::new(AtomicBool::new(true));
    let writer = {
        let lock = lock.clone();
        let writing = Arc::clone(&writing);
        std::thread::spawn(move || {
            let mut held = 1usize;
            while writing.load(Ordering::Relaxed) {
                held += 1;
                let _ = std::fs::write(&lock, vec![b'w'; held]);
                std::thread::sleep(Duration::from_millis(10));
            }
        })
    };

    let out = peer.sync_once();
    writing.store(false, Ordering::Relaxed);
    writer.join().expect("the writer thread");

    assert!(
        lock.exists(),
        "a lock a live writer holds must survive the pass untouched"
    );
    assert!(
        !out.status.success(),
        "the pass must report the ordinary lock error rather than steal the lock; \
         stdout={:?}",
        String::from_utf8_lossy(&out.stdout).trim()
    );
    assert!(
        !String::from_utf8_lossy(&out.stderr)
            .contains("released a reference lock left behind by a killed run"),
        "nothing was recovered, so nothing may claim to have been"
    );
    assert_eq!(
        peer.remote_files(),
        published,
        "a refused commit must publish nothing"
    );
}

#[test]
fn a_branch_lock_a_live_writer_holds_never_hangs_the_daemon() {
    if !git_available() {
        return;
    }

    // The other shape of the same defect, and the expensive one. gitoxide does
    // not fail on a held *branch* lock the way it fails on a held `HEAD.lock`:
    // `gix-ref` 0.66.0 walks the failed edit's parent chain without advancing
    // its cursor (`store/file/transaction/prepare.rs:398-410`), so a child
    // edit's lock failure becomes an infinite loop on a full core, with no
    // error and no output. That is how one CI run spent ninety-six minutes on
    // this file, and on a user's machine it is a wedged, hot process for as
    // long as their own `git commit` holds the lock.
    //
    // Recovery cannot help here — a lock a live writer holds is precisely the
    // one it must leave alone — so the commit path refuses to enter the
    // transaction instead. This asserts the daemon comes back.
    let peer = Peer::new("nohang", 4, 0);
    assert!(
        peer.sync_once().status.success(),
        "the first pass must publish, or the fixture proves nothing"
    );
    std::fs::write(peer.work.join("blocked.txt"), b"waits for the other writer")
        .expect("seed a change so the pass reaches the commit path");

    let lock = peer.work.join(".git").join("refs/heads/main.lock");
    std::fs::write(&lock, b"w").expect("plant the branch lock");
    let writing = Arc::new(AtomicBool::new(true));
    let writer = {
        let lock = lock.clone();
        let writing = Arc::clone(&writing);
        std::thread::spawn(move || {
            let mut held = 1usize;
            while writing.load(Ordering::Relaxed) {
                held += 1;
                let _ = std::fs::write(&lock, vec![b'w'; held]);
                std::thread::sleep(Duration::from_millis(10));
            }
        })
    };

    let mut child = peer
        .daemon(&["sync", "--once"])
        .spawn()
        .expect("spawn a sync pass");
    // Bounded by the test itself rather than by the harness. A regression here
    // is a hang, and the whole point is that a hang must produce a verdict:
    // waiting for nextest's four-minute axe would report a timeout without
    // naming what caused it.
    let deadline = Instant::now() + Duration::from_secs(30);
    let status = loop {
        match child.try_wait().expect("poll the pass") {
            Some(status) => break Some(status),
            None if Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
            None => std::thread::sleep(Duration::from_millis(25)),
        }
    };
    writing.store(false, Ordering::Relaxed);
    writer.join().expect("the writer thread");

    let status = status.expect(
        "the pass must return while another writer holds the branch lock, \
         not spin on it",
    );
    assert!(
        !status.success(),
        "a refused pass is a failure the scheduler retries, not a success"
    );
    assert!(
        lock.exists(),
        "the other writer's lock must survive the refusal"
    );
    assert!(
        !peer.remote_files().iter().any(|path| path == "blocked.txt"),
        "a refused commit must publish nothing"
    );
}
