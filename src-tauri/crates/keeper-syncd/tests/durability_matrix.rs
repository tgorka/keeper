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
use std::process::{Command, Stdio};
use std::time::Duration;

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

        // A kill early enough that the repository was never created leaves
        // nothing for git to check. The user's own files still must be
        // untouched, which is asserted below either way — but running fsck
        // against a plain directory would fail the test for the wrong reason.
        if !self.work.join(".git").exists() {
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
