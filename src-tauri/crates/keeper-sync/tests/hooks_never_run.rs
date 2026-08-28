//! A repository's own hooks never gate keeper's git shim.
//!
//! Field incident, 2026-08-12: a `git lfs install` run by hand inside a synced
//! folder installed stock git-lfs hooks. Every one of them exits non-zero
//! unless `git-lfs` is on `PATH`, and a desktop launch inherits Finder's `PATH`
//! rather than a shell's — so `pre-push` failed and the profile stopped
//! publishing, with a message about a binary keeper does not use. keeper is
//! itself the LFS implementation for a folder it manages, so those hooks were
//! wrong even where git-lfs was installed.
//!
//! This drives the real binary against a real hook, because the whole defect
//! lived in what the spawned process inherited — which no argument-vector
//! assertion can see.

use std::path::{Path, PathBuf};

use keeper_sync::git::GitCli;

fn run(dir: &Path, args: &[&str]) {
    let status = std::process::Command::new("git")
        .args(args)
        .current_dir(dir)
        .status()
        .expect("run git");
    assert!(status.success(), "git {args:?} failed");
}

/// A hook that fails the way git-lfs's does: refuse, loudly, exit non-zero.
#[cfg(unix)]
fn plant_refusing_hook(git_dir: &Path, name: &str) {
    use std::os::unix::fs::PermissionsExt;

    let hooks = git_dir.join("hooks");
    std::fs::create_dir_all(&hooks).expect("hooks directory");
    let path = hooks.join(name);
    std::fs::write(
        &path,
        "#!/bin/sh\nprintf >&2 'this repository is configured for Git LFS but \
         '\\''git-lfs'\\'' was not found on your path\\n'\nexit 2\n",
    )
    .expect("write the hook");
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).expect("make it run");
}

/// An origin to push to, and a clone with one commit ready to go.
fn origin_and_clone(root: &Path) -> (PathBuf, PathBuf) {
    let origin = root.join("origin.git");
    std::fs::create_dir_all(&origin).expect("origin directory");
    run(root, &["init", "-q", "--bare", "-b", "main", "origin.git"]);

    let work = root.join("work");
    std::fs::create_dir_all(&work).expect("work directory");
    run(root, &["init", "-q", "-b", "main", "work"]);
    // Hermetic against the host's own git config. A global `core.hooksPath`
    // (this box has one: `~/.config/git/hooks`) makes every repo-local
    // `.git/hooks/*` inert, so the sanity check below would find nothing to
    // block the plain push and the test would pass while proving nothing.
    // A repository-local `core.hooksPath` outranks the global one, so pointing
    // it at this repository's own hooks directory restores git's default
    // behaviour whatever the host believes. It is also the honest fixture for
    // this defect: the field incident was a repository whose hooks git *did*
    // intend to run. keeper still overrides it, because `GitCli` passes
    // `-c core.hooksPath=<sentinel>` on the command line, which outranks
    // repo-local config in turn — so nothing here softens the real assertion.
    let hooks = work.join(".git").join("hooks");
    let hooks_arg = hooks.to_str().expect("a tempdir path is UTF-8");
    run(&work, &["config", "core.hooksPath", hooks_arg]);
    let origin_arg = origin.to_str().expect("a tempdir path is UTF-8");
    run(&work, &["remote", "add", "origin", origin_arg]);
    std::fs::write(work.join("a.txt"), b"one\n").expect("write");
    run(&work, &["add", "a.txt"]);
    run(
        &work,
        &[
            "-c",
            "user.email=t@e",
            "-c",
            "user.name=t",
            "commit",
            "-q",
            "-m",
            "one",
        ],
    );
    (origin, work)
}

#[test]
#[cfg(unix)]
fn a_refusing_pre_push_hook_does_not_stop_a_push() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (origin, work) = origin_and_clone(dir.path());
    plant_refusing_hook(&work.join(".git"), "pre-push");

    // Proof the hook is real and would bite: plain git cannot push past it.
    let blocked = std::process::Command::new("git")
        .args(["push", "origin", "refs/heads/main:refs/heads/main"])
        .current_dir(&work)
        .output()
        .expect("run git");
    assert!(
        !blocked.status.success(),
        "the planted hook must actually block a plain push, or this test proves nothing"
    );

    let cli = GitCli::new(PathBuf::from("git"));
    cli.push(&work, "origin", "refs/heads/main:refs/heads/main", None)
        .expect("keeper's push must not run the repository's hooks");

    let remote = std::process::Command::new("git")
        .args(["rev-parse", "refs/heads/main"])
        .current_dir(&origin)
        .output()
        .expect("rev-parse");
    assert!(
        remote.status.success(),
        "the push must have actually landed on the remote"
    );
}
