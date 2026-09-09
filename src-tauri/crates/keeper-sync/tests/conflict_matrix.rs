//! The conflict matrix, driven through `Engine::sync_once` against the real
//! `git` (Story 70.3, AD-229, AD-43).
//!
//! `git/conflict.rs` proves the matrix as a pure function and did so for two
//! epics while nothing called it. What the review measured (`pullpush.md`,
//! F-PULLPUSH-1/-2/-13) is that keeper's real merge vector exits 1 on
//! modify/delete, rename-vs-modify, file/directory and case-only, leaves
//! `MERGE_HEAD`, and livelocks the profile at exit 128 for good. So every row
//! here is a **real repository, a real bare remote, a real peer clone**, and
//! the assertion is made after the engine's own pull leg has run: exit 0, no
//! `MERGE_HEAD`, the tree AD-43 predicts, and the conflict copy — where one
//! is due — inside the merge commit rather than left for a later pass.
//!
//! **Local commits are made by hand.** `commit_local` is what makes the tree
//! clean before a merge, and a settle-gated commit of the local edit would
//! push it before the remote could move; committing with `git` directly is
//! what lets both sides diverge from one base on purpose. The engine's own
//! commit path is Story 70.5's to prove.
//!
//! **The clock is injected** and never advanced unless a test says so, which
//! is what makes "two passes within one second" a deterministic case rather
//! than a race.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;

use keeper_sync::engine::{Engine, SyncOutcome, MERGE_IN_PROGRESS_SENTENCE};
use keeper_sync::error::SyncError;
use keeper_sync::platform::{SyncPlatform, TestPlatform};
use keeper_sync::profile::SyncProfile;
use keeper_sync::provenance::SyncSource;

const PROFILE_ID: &str = "01JMATRIX";

/// Run one `git` in `cwd`, panicking on failure with git's own words.
fn git(cwd: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .args(args)
        .current_dir(cwd)
        // Hooks and identity from this box must not shape a fixture.
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@example.invalid")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@example.invalid")
        .output()
        .expect("git");
    assert!(
        out.status.success(),
        "git {args:?} in {}: {}{}",
        cwd.display(),
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// [`git`] whose exit status is the answer.
fn git_ok(cwd: &Path, args: &[&str]) -> bool {
    Command::new("git")
        .args(args)
        .current_dir(cwd)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@example.invalid")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@example.invalid")
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

fn write(path: &Path, bytes: &[u8]) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("mkdir");
    }
    std::fs::write(path, bytes).expect("write");
}

/// Root, bare remote, a peer clone, the engine and its clock.
struct Fixture {
    _work: tempfile::TempDir,
    _remote: tempfile::TempDir,
    _peer: tempfile::TempDir,
    _data: tempfile::TempDir,
    root: PathBuf,
    peer: PathBuf,
    engine: Engine,
    platform: Arc<TestPlatform>,
}

/// Build the fixture with `base` committed and published, and the peer
/// cloned from the remote. `None` on a machine with no usable `git`.
async fn fixture(base: &[(&str, &[u8])]) -> Option<Fixture> {
    let work = tempfile::tempdir().expect("tempdir");
    let remote = tempfile::tempdir().expect("remote");
    let peer = tempfile::tempdir().expect("peer");
    let data = tempfile::tempdir().expect("data");
    if gix::init_bare(remote.path()).is_err() {
        return None;
    }
    let root = work.path().to_path_buf();
    if !git_ok(&root, &["init", "-q", "-b", "main"]) {
        return None;
    }
    git(
        &root,
        &["remote", "add", "origin", &remote.path().to_string_lossy()],
    );
    for (name, bytes) in base {
        write(&root.join(name), bytes);
    }
    git(&root, &["add", "-A"]);
    git(&root, &["commit", "-qm", "base"]);

    let platform = Arc::new(TestPlatform::new(data.path()));
    let engine = Engine::open(Arc::clone(&platform) as Arc<dyn SyncPlatform>).ok()?;
    let profile = SyncProfile::new(
        PROFILE_ID,
        "matrix",
        &root,
        remote.path().to_string_lossy().into_owned(),
    );
    engine.upsert_profile(&profile).expect("register");
    // Publishes the base so both sides share an ancestor.
    engine
        .sync_once(PROFILE_ID, SyncSource::Manual)
        .await
        .expect("the base is published to the bare remote");

    let peer_root = peer.path().join("clone");
    git(
        peer.path(),
        &["clone", "-q", &remote.path().to_string_lossy(), "clone"],
    );
    Some(Fixture {
        _work: work,
        _remote: remote,
        _peer: peer,
        _data: data,
        root,
        peer: peer_root,
        engine,
        platform,
    })
}

impl Fixture {
    /// Commit whatever the peer's tree holds and push it: the remote moved.
    fn peer_publishes(&self, message: &str) {
        git(&self.peer, &["add", "-A"]);
        git(&self.peer, &["commit", "-qm", message]);
        git(&self.peer, &["push", "-q", "origin", "main"]);
    }

    /// Commit whatever the local tree holds, by hand: we moved too.
    fn local_commits(&self, message: &str) {
        git(&self.root, &["add", "-A"]);
        git(&self.root, &["commit", "-qm", message]);
    }

    async fn sync(&self) -> Result<SyncOutcome, SyncError> {
        self.engine.sync_once(PROFILE_ID, SyncSource::Manual).await
    }

    fn merge_head(&self) -> PathBuf {
        self.root.join(".git/MERGE_HEAD")
    }

    fn read(&self, rela: &str) -> Vec<u8> {
        std::fs::read(self.root.join(rela)).unwrap_or_else(|err| panic!("{rela}: {err}"))
    }

    /// Paths the merge commit touched, from git's own record of it.
    fn in_head_commit(&self) -> Vec<String> {
        git(&self.root, &["show", "--name-only", "--format=", "HEAD"])
            .lines()
            .map(str::to_owned)
            .filter(|line| !line.is_empty())
            .collect()
    }

    fn tracked(&self) -> Vec<String> {
        git(&self.root, &["ls-files"])
            .lines()
            .map(str::to_owned)
            .collect()
    }

    fn unmerged(&self) -> String {
        git(&self.root, &["ls-files", "--unmerged"])
    }

    /// The one conflict copy of `stem` in `dir`, or a panic naming what is
    /// there instead.
    fn copy_of(&self, dir: &str, stem: &str) -> String {
        let mut found = self.copies_of(dir, stem);
        assert_eq!(found.len(), 1, "exactly one copy of {stem}: {found:?}");
        found.remove(0)
    }

    fn copies_of(&self, dir: &str, stem: &str) -> Vec<String> {
        let prefix = format!("{stem}.sync-conflict-");
        let mut found: Vec<String> = std::fs::read_dir(self.root.join(dir))
            .expect("read dir")
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .filter(|name| name.starts_with(&prefix))
            .map(|name| {
                if dir.is_empty() {
                    name
                } else {
                    format!("{dir}/{name}")
                }
            })
            .collect();
        found.sort();
        found
    }

    /// Every assertion a finished merge must satisfy, whatever the row.
    fn assert_finished(&self, outcome: &SyncOutcome) {
        assert!(
            !self.merge_head().exists(),
            "MERGE_HEAD must not survive a pass: {outcome:?}"
        );
        assert_eq!(self.unmerged(), "", "nothing may stay unmerged");
        assert!(
            outcome.pulled,
            "the remote moved, so the pass pulled: {outcome:?}"
        );
        let parents = git(&self.root, &["rev-list", "--parents", "-1", "HEAD"]);
        assert_eq!(
            parents.split_whitespace().count(),
            3,
            "HEAD is the merge commit, with both parents: {parents}"
        );
        let message = git(&self.root, &["log", "-1", "--format=%B"]);
        assert!(
            message.contains("Keeper-Profile: matrix"),
            "the merge commit carries keeper's provenance: {message}"
        );
        assert!(
            !self.tracked().iter().any(|path| path.contains('~')),
            "no rescue litter may be tracked: {:?}",
            self.tracked()
        );
    }
}

// --- the measured rows ------------------------------------------------------

#[tokio::test]
async fn local_delete_remote_modify_takes_the_modification() {
    let Some(f) = fixture(&[("f.txt", b"base")]).await else {
        return;
    };
    write(&f.peer.join("f.txt"), b"theirs");
    f.peer_publishes("modify");
    std::fs::remove_file(f.root.join("f.txt")).expect("delete");
    f.local_commits("delete");

    let outcome = f
        .sync()
        .await
        .expect("modify beats delete, without a human");
    f.assert_finished(&outcome);
    assert_eq!(f.read("f.txt"), b"theirs", "the modification wins (AD-43)");
    assert!(outcome.conflicts.is_empty(), "a delete has no copy to keep");
    assert!(f.copies_of("", "f").is_empty());
}

#[tokio::test]
async fn local_modify_remote_delete_keeps_the_modification() {
    let Some(f) = fixture(&[("f.txt", b"base")]).await else {
        return;
    };
    std::fs::remove_file(f.peer.join("f.txt")).expect("delete");
    f.peer_publishes("delete");
    write(&f.root.join("f.txt"), b"ours");
    f.local_commits("modify");

    let outcome = f
        .sync()
        .await
        .expect("modify beats delete, without a human");
    f.assert_finished(&outcome);
    assert_eq!(f.read("f.txt"), b"ours", "the modification wins (AD-43)");
    assert!(outcome.conflicts.is_empty());
    assert!(f.tracked().contains(&"f.txt".to_owned()));
}

#[tokio::test]
async fn a_rename_against_a_modify_keeps_both_names() {
    let Some(f) = fixture(&[("a.txt", b"base")]).await else {
        return;
    };
    write(&f.peer.join("a.txt"), b"theirs");
    f.peer_publishes("modify a");
    git(&f.root, &["mv", "a.txt", "b.txt"]);
    f.local_commits("rename a to b");

    let outcome = f.sync().await.expect("a rename is a delete and an add");
    f.assert_finished(&outcome);
    assert_eq!(
        f.read("a.txt"),
        b"theirs",
        "the old path takes the remote's edit"
    );
    assert_eq!(f.read("b.txt"), b"base", "the new path keeps ours");
    assert!(outcome.conflicts.is_empty());
}

#[tokio::test]
async fn a_file_against_a_directory_becomes_a_copy_beside_the_directory() {
    let Some(f) = fixture(&[("base.txt", b"base")]).await else {
        return;
    };
    write(&f.peer.join("d/x.txt"), b"theirs");
    f.peer_publishes("add d/x.txt");
    write(&f.root.join("d"), b"ours");
    f.local_commits("add file d");

    let outcome = f
        .sync()
        .await
        .expect("a file/directory collision is resolved");
    f.assert_finished(&outcome);
    assert_eq!(f.read("d/x.txt"), b"theirs", "the directory keeps the name");
    let copy = f.copy_of("", "d");
    assert_eq!(f.read(&copy), b"ours", "our file survives as the copy");
    assert!(
        !f.root.join("d~HEAD").exists(),
        "git's rescue file is not left as litter"
    );
    assert_eq!(outcome.conflicts, vec![copy.clone()], "and it is reported");
    assert!(
        f.in_head_commit().contains(&copy),
        "the copy is in the merge commit: {:?}",
        f.in_head_commit()
    );
}

#[tokio::test]
async fn a_directory_against_a_file_keeps_the_directory_and_copies_the_file() {
    let Some(f) = fixture(&[("base.txt", b"base")]).await else {
        return;
    };
    write(&f.peer.join("d"), b"theirs");
    f.peer_publishes("add file d");
    write(&f.root.join("d/x.txt"), b"ours");
    f.local_commits("add d/x.txt");

    let outcome = f
        .sync()
        .await
        .expect("a directory/file collision is resolved");
    f.assert_finished(&outcome);
    assert_eq!(
        f.read("d/x.txt"),
        b"ours",
        "the directory cannot move aside"
    );
    let copy = f.copy_of("", "d");
    assert_eq!(f.read(&copy), b"theirs", "their file survives as the copy");
    let litter: Vec<String> = std::fs::read_dir(&f.root)
        .expect("root")
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| name.contains('~'))
        .collect();
    assert!(litter.is_empty(), "no rescue litter: {litter:?}");
    assert!(f.in_head_commit().contains(&copy));
}

#[tokio::test]
async fn a_case_only_rename_under_ignorecase_finishes_the_merge() {
    let Some(f) = fixture(&[("f.txt", b"base")]).await else {
        return;
    };
    // hesperia's configuration (evidence line 41): the shape a Linux peer's
    // case-only rename arrives in on a Mac.
    git(&f.root, &["config", "core.ignorecase", "true"]);
    write(&f.peer.join("f.txt"), b"theirs");
    f.peer_publishes("modify f");
    git(&f.root, &["mv", "f.txt", "F.txt"]);
    f.local_commits("rename f to F");

    let outcome = f
        .sync()
        .await
        .expect("a case-only rename reads as modify/delete and resolves");
    f.assert_finished(&outcome);
    let tracked = f.tracked();
    assert!(tracked.contains(&"F.txt".to_owned()), "ours: {tracked:?}");
    assert!(tracked.contains(&"f.txt".to_owned()), "theirs: {tracked:?}");
    assert_eq!(f.read("f.txt"), b"theirs");
}

#[tokio::test]
async fn add_add_keeps_the_remote_at_the_path_and_ours_beside_it_in_the_same_commit() {
    let Some(f) = fixture(&[("base.txt", b"base")]).await else {
        return;
    };
    write(&f.peer.join("new.txt"), b"theirs");
    f.peer_publishes("add new");
    write(&f.root.join("new.txt"), b"ours");
    f.local_commits("add new too");

    let outcome = f.sync().await.expect("add/add converges");
    f.assert_finished(&outcome);
    assert_eq!(f.read("new.txt"), b"theirs");
    let copy = f.copy_of("", "new");
    assert_eq!(f.read(&copy), b"ours");
    assert_eq!(outcome.conflicts, vec![copy.clone()]);
    let committed = f.in_head_commit();
    assert!(committed.contains(&copy), "{committed:?}");
    assert_eq!(
        git(&f.root, &["status", "--porcelain"]).trim(),
        "",
        "nothing is left for a later pass to commit"
    );
}

#[tokio::test]
async fn delete_delete_needs_nothing() {
    let Some(f) = fixture(&[("gone.txt", b"base"), ("kept.txt", b"kept")]).await else {
        return;
    };
    std::fs::remove_file(f.peer.join("gone.txt")).expect("delete");
    f.peer_publishes("delete");
    std::fs::remove_file(f.root.join("gone.txt")).expect("delete");
    f.local_commits("delete too");

    let outcome = f.sync().await.expect("agreeing deletions merge cleanly");
    f.assert_finished(&outcome);
    assert!(!f.root.join("gone.txt").exists());
    assert!(outcome.conflicts.is_empty());
    assert!(f.copies_of("", "gone").is_empty());
}

#[tokio::test]
async fn modify_modify_text_binary_and_pointer_each_keep_a_copy_in_the_merge_commit() {
    let pointer = |oid: &str| {
        format!("version https://git-lfs.github.com/spec/v1\noid sha256:{oid}\nsize 4096\n")
    };
    let base_pointer = pointer(&"a".repeat(64));
    let base_bin = [0u8, 1, 2, 0, 255, 254, 0, 7];
    let Some(f) = fixture(&[
        ("notes/t.txt", b"base"),
        ("b.bin", &base_bin),
        ("big.mov", base_pointer.as_bytes()),
    ])
    .await
    else {
        return;
    };
    write(&f.peer.join("notes/t.txt"), b"theirs");
    write(&f.peer.join("b.bin"), &[0u8, 9, 9, 0, 255, 0, 1]);
    write(&f.peer.join("big.mov"), pointer(&"b".repeat(64)).as_bytes());
    f.peer_publishes("modify three");
    write(&f.root.join("notes/t.txt"), b"ours");
    write(&f.root.join("b.bin"), &[0u8, 3, 3, 0, 254, 0, 2]);
    write(&f.root.join("big.mov"), pointer(&"c".repeat(64)).as_bytes());
    f.local_commits("modify three too");

    let outcome = f.sync().await.expect("content conflicts converge");
    f.assert_finished(&outcome);
    assert_eq!(f.read("notes/t.txt"), b"theirs");
    assert_eq!(f.read("b.bin"), [0u8, 9, 9, 0, 255, 0, 1]);
    assert_eq!(f.read("big.mov"), pointer(&"b".repeat(64)).as_bytes());

    let text = f.copy_of("notes", "t");
    let bin = f.copy_of("", "b");
    let mov = f.copy_of("", "big");
    assert_eq!(f.read(&text), b"ours");
    assert_eq!(f.read(&bin), [0u8, 3, 3, 0, 254, 0, 2]);
    assert_eq!(f.read(&mov), pointer(&"c".repeat(64)).as_bytes());
    let mut expected = vec![text.clone(), bin.clone(), mov.clone()];
    expected.sort();
    assert_eq!(outcome.conflicts, expected);
    let committed = f.in_head_commit();
    for copy in [&text, &bin, &mov] {
        assert!(committed.contains(copy), "{copy} in {committed:?}");
    }
}

// --- the guards -------------------------------------------------------------

#[tokio::test]
async fn a_merge_an_old_keeper_left_in_progress_is_undone_and_the_folder_syncs_again() {
    let Some(f) = fixture(&[("f.txt", b"base")]).await else {
        return;
    };
    write(&f.peer.join("f.txt"), b"theirs");
    f.peer_publishes("modify");
    std::fs::remove_file(f.root.join("f.txt")).expect("delete");
    f.local_commits("delete");
    // The state the field folder was found in: keeper's own vector, run
    // without an abort, leaving MERGE_HEAD and an unmerged index.
    git(&f.root, &["fetch", "-q", "origin"]);
    assert!(!git_ok(
        &f.root,
        &[
            "merge",
            "--no-edit",
            "--quiet",
            "-s",
            "ort",
            "-X",
            "theirs",
            "-X",
            "no-renames",
            "-m",
            "m",
            "refs/remotes/origin/main",
        ],
    ));
    assert!(f.merge_head().is_file(), "arranged: a merge is in progress");
    assert_ne!(f.unmerged(), "", "arranged: the index is unmerged");

    let outcome = f
        .sync()
        .await
        .expect("the stale merge is aborted and the pass converges");
    f.assert_finished(&outcome);
    assert_eq!(f.read("f.txt"), b"theirs");
}

#[tokio::test]
async fn a_merge_head_that_will_not_abort_refuses_by_name_and_commits_nothing() {
    let Some(f) = fixture(&[("f.txt", b"base")]).await else {
        return;
    };
    // Measured on git 2.53: `merge --abort` exits 0 and leaves a MERGE_HEAD
    // that is a directory standing — the one shape the abort cannot clear.
    write(&f.root.join("f.txt"), b"edited");
    f.local_commits("edit");
    let head_before = git(&f.root, &["rev-parse", "HEAD"]);
    std::fs::create_dir(f.merge_head()).expect("plant");
    // A settled local edit the engine would otherwise commit.
    write(&f.root.join("g.txt"), b"new");

    let err = f
        .sync()
        .await
        .expect_err("keeper never tree-builds over a merge in progress");
    let text = err.to_string();
    assert!(
        text.contains(MERGE_IN_PROGRESS_SENTENCE),
        "the refusal is the sentence: {text}"
    );
    assert!(text.contains("MERGE_HEAD"), "and names the file: {text}");
    assert_eq!(git(&f.root, &["rev-parse", "HEAD"]), head_before);
}

#[tokio::test]
async fn two_passes_within_one_second_keep_both_copies() {
    let Some(f) = fixture(&[("f.txt", b"base")]).await else {
        return;
    };
    write(&f.peer.join("f.txt"), b"theirs 1");
    f.peer_publishes("modify 1");
    write(&f.root.join("f.txt"), b"ours 1");
    f.local_commits("modify 1 too");
    let first = f.sync().await.expect("first divergence");
    f.assert_finished(&first);
    let first_copy = f.copy_of("", "f");

    // The clock is the platform's and has not moved: same stamp, same path.
    git(&f.peer, &["pull", "-q", "origin", "main"]);
    write(&f.peer.join("f.txt"), b"theirs 2");
    f.peer_publishes("modify 2");
    write(&f.root.join("f.txt"), b"ours 2");
    f.local_commits("modify 2 too");
    let second = f.sync().await.expect("second divergence");
    f.assert_finished(&second);

    let copies = f.copies_of("", "f");
    assert_eq!(copies.len(), 2, "both copies stand: {copies:?}");
    assert_eq!(
        f.read(&first_copy),
        b"ours 1",
        "the first copy was not truncated"
    );
    let second_copy = copies
        .iter()
        .find(|copy| **copy != first_copy)
        .expect("a second name");
    assert_eq!(f.read(second_copy), b"ours 2");
    assert_eq!(second.conflicts, vec![second_copy.clone()]);
    assert!(f.in_head_commit().contains(second_copy));
}

/// The merge resolves, and then the commit dies with `MERGE_HEAD` standing —
/// the shape a `.git` with debris in it (or a child killed at its deadline)
/// produces. Measured: `COMMIT_EDITMSG` as a directory makes `git commit`
/// exit 128 after the merge went well. The pass must undo the merge, keep no
/// copy, and the next pass — debris gone — must finish it.
#[tokio::test]
async fn a_merge_whose_commit_dies_is_undone_with_its_copies_and_finishes_next_pass() {
    let Some(f) = fixture(&[("f.txt", b"base")]).await else {
        return;
    };
    write(&f.peer.join("f.txt"), b"theirs");
    f.peer_publishes("modify");
    write(&f.root.join("f.txt"), b"ours");
    f.local_commits("modify too");
    let debris = f.root.join(".git/COMMIT_EDITMSG");
    let _ = std::fs::remove_file(&debris);
    std::fs::create_dir(&debris).expect("plant the debris");

    let err = f.sync().await.expect_err("the commit cannot be written");
    assert!(
        !f.merge_head().exists(),
        "a merge that does not finish is undone: {err}"
    );
    assert_eq!(f.unmerged(), "");
    assert_eq!(f.read("f.txt"), b"ours", "the worktree is back at HEAD");
    assert!(
        f.copies_of("", "f").is_empty(),
        "the copy made for the undone merge went with it"
    );

    std::fs::remove_dir(&debris).expect("clear the debris");
    f.platform.advance_ms(1_000);
    let outcome = f
        .sync()
        .await
        .expect("the next pass meets a clean repository");
    f.assert_finished(&outcome);
    assert_eq!(f.read("f.txt"), b"theirs");
    let copy = f.copy_of("", "f");
    assert_eq!(f.read(&copy), b"ours");
    assert!(f.in_head_commit().contains(&copy));
}

/// Linux only: APFS refuses a non-UTF-8 name outright (`EILSEQ`, "Illegal
/// byte sequence"), so on macOS the fixture cannot be written and the case
/// cannot arise there — the copy-naming code is the same on both.
#[cfg(target_os = "linux")]
#[tokio::test]
async fn a_non_utf8_path_gets_its_own_copy() {
    use std::os::unix::ffi::{OsStrExt as _, OsStringExt as _};
    let odd = std::ffi::OsString::from_vec(b"na\xFFme.txt".to_vec());
    let Some(f) = fixture(&[("base.txt", b"base")]).await else {
        return;
    };
    write(&f.peer.join(&odd), b"theirs");
    f.peer_publishes("add odd");
    write(&f.root.join(&odd), b"ours");
    f.local_commits("add odd too");

    let outcome = f
        .sync()
        .await
        .expect("a name that is not UTF-8 is still a name");
    f.assert_finished(&outcome);
    assert_eq!(
        std::fs::read(f.root.join(&odd)).expect("canonical"),
        b"theirs"
    );
    let copies: Vec<std::ffi::OsString> = std::fs::read_dir(&f.root)
        .expect("root")
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.file_name())
        .filter(|name| name.as_bytes().starts_with(b"na\xFFme.sync-conflict-"))
        .collect();
    assert_eq!(
        copies.len(),
        1,
        "one copy, named from the bytes: {copies:?}"
    );
    assert!(copies[0].as_bytes().ends_with(b".txt"));
    assert_eq!(
        std::fs::read(f.root.join(&copies[0])).expect("copy"),
        b"ours"
    );
    assert_eq!(outcome.conflicts.len(), 1);
    // In the merge commit: git records the raw bytes, and `show` quotes them.
    let committed = git(
        &f.root,
        &[
            "-c",
            "core.quotePath=false",
            "show",
            "--name-only",
            "--format=",
            "HEAD",
        ],
    );
    assert!(
        committed.contains(".sync-conflict-"),
        "the copy is in the merge commit: {committed}"
    );
}

#[cfg(unix)]
#[tokio::test]
async fn a_resolution_that_cannot_be_written_is_undone_and_the_next_pass_finishes_it() {
    use std::os::unix::fs::PermissionsExt as _;
    let Some(f) = fixture(&[("d/f.txt", b"base")]).await else {
        return;
    };
    write(&f.peer.join("d/f.txt"), b"theirs");
    f.peer_publishes("modify");
    std::fs::remove_file(f.root.join("d/f.txt")).expect("delete");
    f.local_commits("delete");
    // `d/` stays, read-only: the merge cannot write their revision into it.
    let dir = f.root.join("d");
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o555)).expect("chmod");
    if std::fs::write(dir.join("probe"), b"").is_ok() {
        // Root ignores the mode; there is nothing to prove here.
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).expect("chmod");
        return;
    }

    let err = f.sync().await.expect_err("the merge cannot finish");
    assert!(
        !f.merge_head().exists(),
        "a merge that does not finish is undone: {err}"
    );
    assert_eq!(f.unmerged(), "", "and leaves no stages behind");
    assert!(f.copies_of("d", "f").is_empty());

    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).expect("chmod");
    f.platform.advance_ms(1_000);
    let outcome = f
        .sync()
        .await
        .expect("the next pass meets a clean repository");
    f.assert_finished(&outcome);
    assert_eq!(f.read("d/f.txt"), b"theirs");
}
