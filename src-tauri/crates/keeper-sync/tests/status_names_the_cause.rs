//! A failing status must say *why* it failed.
//!
//! Field incident, 2026-08-12: a profile spent hours reporting only
//! "git object store failure: status failed: IO error while writing blob or
//! reading file metadata or changing filetype" — gitoxide's top frame — while
//! the actual cause (an unreadable tracked file) sat two `source()` hops down
//! and was never printed. gix does not name the path either, so the flattened
//! cause chain is the only diagnostic a report from the field can carry.

use keeper_sync::git;

fn git(root: &std::path::Path, args: &[&str]) {
    let status = std::process::Command::new("git")
        .args(args)
        .current_dir(root)
        .status()
        .expect("run git");
    assert!(status.success(), "git {args:?} failed");
}

#[test]
#[cfg(unix)]
fn a_status_error_carries_the_io_cause() {
    use std::os::unix::fs::PermissionsExt;

    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    git(root, &["init", "-q", "-b", "main"]);
    std::fs::write(root.join("tracked.txt"), b"v1\n").expect("write");
    git(root, &["add", "tracked.txt"]);
    git(
        root,
        &[
            "-c",
            "user.email=t@e",
            "-c",
            "user.name=t",
            "commit",
            "-q",
            "-m",
            "x",
        ],
    );

    // Same length, different bytes: the stat alone cannot settle whether the
    // path changed, so status has to open the file — and cannot.
    std::fs::write(root.join("tracked.txt"), b"v2\n").expect("rewrite");
    std::fs::set_permissions(
        root.join("tracked.txt"),
        std::fs::Permissions::from_mode(0o000),
    )
    .expect("revoke read");

    let repo = git::repo::open(root, false).expect("open");
    let err = git::repo::status_paths(&repo).expect_err("an unreadable tracked file fails status");

    let message = err.to_string();
    assert!(
        message.contains("os error 13") || message.to_lowercase().contains("permission denied"),
        "the message must carry the underlying cause, got: {message}"
    );

    // Restore so the tempdir cleanup (and any rerun on a kept dir) can read it.
    std::fs::set_permissions(
        root.join("tracked.txt"),
        std::fs::Permissions::from_mode(0o644),
    )
    .expect("restore permissions");
}
