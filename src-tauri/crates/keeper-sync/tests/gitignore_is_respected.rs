//! `.gitignore` decides what syncs, including inside a brand-new folder.
//!
//! The bug this pins: gitoxide's dirwalk defaults to `CollapseDirectory`, so a
//! new folder arrived as ONE untracked entry naming the directory, and keeper
//! expanded it with its own filesystem walk. That walk knew nothing about
//! ignore rules, so an ignored file inside a NEW directory was staged and
//! pushed while the identical file one level up was correctly skipped — a
//! folder holding `node_modules/`, build output or a `.env` reached the remote
//! in full. The status walk now emits untracked content file by file, which
//! hands the judgement back to the only thing that reads every `.gitignore`,
//! `.git/info/exclude` and the global excludes file correctly.

use std::path::Path;
use std::process::Command;

fn git(dir: &Path, args: &[&str]) {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_AUTHOR_NAME", "keeper-test")
        .env("GIT_AUTHOR_EMAIL", "test@keeper.invalid")
        .env("GIT_COMMITTER_NAME", "keeper-test")
        .env("GIT_COMMITTER_EMAIL", "test@keeper.invalid")
        .output()
        .expect("git runs");
    assert!(
        out.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn an_ignored_file_is_skipped_wherever_it_lives() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    git(root, &["init", "-q", "-b", "main"]);
    std::fs::write(root.join(".gitignore"), "*.log\nsecrets/\n").expect("write .gitignore");
    git(root, &["add", ".gitignore"]);
    git(root, &["commit", "-qm", "seed"]);

    // At a level git already knows about: this case always worked.
    std::fs::write(root.join("top.txt"), b"keep me").expect("write");
    std::fs::write(root.join("top.log"), b"ignored").expect("write");

    // Inside a brand-new folder: this is the case that leaked.
    std::fs::create_dir_all(root.join("newdir/nested")).expect("mkdir");
    std::fs::write(root.join("newdir/keep.txt"), b"keep me").expect("write");
    std::fs::write(root.join("newdir/skip.log"), b"ignored").expect("write");
    std::fs::write(root.join("newdir/nested/deep.log"), b"ignored").expect("write");
    std::fs::write(root.join("newdir/nested/deep.txt"), b"keep me").expect("write");

    // A wholly-ignored directory, new as well.
    std::fs::create_dir_all(root.join("secrets")).expect("mkdir");
    std::fs::write(root.join("secrets/token.txt"), b"never").expect("write");

    let repo = keeper_sync::git::repo::open(root, false).expect("open");
    let status = keeper_sync::git::repo::status_paths(&repo).expect("status");
    let mut got: Vec<String> = status
        .untracked
        .iter()
        .map(|p| p.to_string_lossy().replace('\\', "/"))
        .collect();
    got.sort();

    assert_eq!(
        got,
        vec![
            "newdir/keep.txt".to_owned(),
            "newdir/nested/deep.txt".to_owned(),
            "top.txt".to_owned(),
        ],
        "only non-ignored files, at any depth, and never a bare directory"
    );

    // The strongest form of the assertion: keeper agrees with git itself.
    let porcelain = Command::new("git")
        .args(["status", "--porcelain", "--untracked-files=all"])
        .current_dir(root)
        .output()
        .expect("git status");
    let mut git_says: Vec<String> = String::from_utf8_lossy(&porcelain.stdout)
        .lines()
        .filter_map(|line| line.strip_prefix("?? "))
        .map(str::to_owned)
        .collect();
    git_says.sort();
    assert_eq!(got, git_says, "keeper's untracked set must be git's own");
}
