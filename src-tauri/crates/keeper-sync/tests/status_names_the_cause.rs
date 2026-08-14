//! A failing status must say *why*, and must not cost the folder its sync.
//!
//! Field incident, 2026-08-11/12, on two machines within a day: a profile
//! reported only "git object store failure: status failed: IO error while
//! writing blob or reading file metadata or changing filetype" — gitoxide's top
//! frame — while the actual cause sat two `source()` hops down and the path was
//! never named at all. On the second machine it repeated for sixteen
//! consecutive passes, with nothing in the folder syncing, because gitoxide
//! aborts the whole walk on a per-entry IO failure.

use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use keeper_sync::git;

fn git_cmd(root: &Path, args: &[&str]) {
    let status = std::process::Command::new("git")
        .args(args)
        .current_dir(root)
        .status()
        .expect("run git");
    assert!(status.success(), "git {args:?} failed");
}

fn commit_all(root: &Path) {
    git_cmd(root, &["add", "-A"]);
    git_cmd(
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
}

fn unreadable(path: &Path) {
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o000)).expect("revoke read");
}

fn readable_again(path: &Path) {
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o644)).expect("restore");
}

#[test]
#[cfg(unix)]
fn an_unreadable_file_is_named_and_stepped_over() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    git_cmd(root, &["init", "-q", "-b", "main"]);

    // The name is deliberately glob syntax. A synced folder is full of these,
    // and an exclusion that was not `literal` would hide unrelated files.
    let awkward = root.join("report [2026] *draft?.txt");
    let ordinary = root.join("ordinary.txt");
    std::fs::write(&awkward, b"v1\n").expect("write");
    std::fs::write(&ordinary, b"v1\n").expect("write");
    commit_all(root);

    // Same length, different bytes: the stat alone cannot settle either path,
    // so status must read both — and can only read one.
    std::fs::write(&awkward, b"v2\n").expect("rewrite");
    std::fs::write(&ordinary, b"v2\n").expect("rewrite");
    unreadable(&awkward);

    let repo = git::repo::open(root, false).expect("open");
    let status = git::repo::status_paths(&repo).expect("the readable rest of the folder must sync");
    readable_again(&awkward);

    assert_eq!(
        status.unreadable.len(),
        1,
        "the unreadable path must be reported, got {:?}",
        status.unreadable
    );
    let reported = &status.unreadable[0];
    assert_eq!(reported.path, Path::new("report [2026] *draft?.txt"));
    assert!(
        reported.reason.contains("os error 13")
            || reported.reason.to_lowercase().contains("permission denied"),
        "the reason must be the errno, got: {}",
        reported.reason
    );

    // The whole point: everything else still converges.
    assert_eq!(
        status.modified,
        vec![std::path::PathBuf::from("ordinary.txt")],
        "the readable file's change must still be reported"
    );
}

#[test]
#[cfg(unix)]
fn a_failure_it_cannot_pin_on_a_path_stays_loud_and_names_its_cause() {
    // An unreadable *untracked* directory fails the dirwalk, not an index
    // entry, so the per-path diagnosis finds nothing to exclude. The original
    // error must survive intact — with the errno the flattened chain carries,
    // rather than gitoxide's top frame alone.
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    git_cmd(root, &["init", "-q", "-b", "main"]);
    std::fs::write(root.join("a.txt"), b"v1\n").expect("write");
    commit_all(root);

    let closed = root.join("closed");
    std::fs::create_dir(&closed).expect("mkdir");
    std::fs::write(closed.join("inner.txt"), b"x\n").expect("write");
    std::fs::set_permissions(&closed, std::fs::Permissions::from_mode(0o000)).expect("revoke");

    let repo = git::repo::open(root, false).expect("open");
    let err = git::repo::status_paths(&repo).expect_err("an unreadable directory fails the walk");
    std::fs::set_permissions(&closed, std::fs::Permissions::from_mode(0o755)).expect("restore");

    let message = err.to_string();
    assert!(
        message.contains("os error 13") || message.to_lowercase().contains("permission denied"),
        "the message must carry the underlying cause, got: {message}"
    );
}

#[test]
#[cfg(unix)]
fn a_file_returns_to_synchronization_the_moment_it_can_be_read() {
    // The diagnosis is remembered so it is not re-walked every pass, which
    // makes forgetting it the load-bearing half: a user who restores a
    // permission must not have to restart anything, and must not be left with a
    // file that is silently excluded forever.
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    git_cmd(root, &["init", "-q", "-b", "main"]);
    let locked = root.join("locked.txt");
    std::fs::write(&locked, b"v1\n").expect("write");
    commit_all(root);
    std::fs::write(&locked, b"v2\n").expect("rewrite");
    unreadable(&locked);

    let repo = git::repo::open(root, false).expect("open");
    let skipped = git::repo::status_paths(&repo).expect("status");
    assert_eq!(
        skipped.unreadable.len(),
        1,
        "the file must be skipped first"
    );
    assert!(
        skipped.modified.is_empty(),
        "a file nobody can read is not a change to stage"
    );

    readable_again(&locked);

    let healed = git::repo::status_paths(&repo).expect("status");
    assert!(
        healed.unreadable.is_empty(),
        "the remembered skip must be dropped once the file can be read"
    );
    assert_eq!(
        healed.modified,
        vec![std::path::PathBuf::from("locked.txt")],
        "and the change it was hiding must now be reported"
    );
}

#[test]
#[cfg(unix)]
fn a_healthy_folder_reports_nothing_unreadable() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    git_cmd(root, &["init", "-q", "-b", "main"]);
    std::fs::write(root.join("a.txt"), b"v1\n").expect("write");
    commit_all(root);
    std::fs::write(root.join("a.txt"), b"v2\n").expect("rewrite");

    let repo = git::repo::open(root, false).expect("open");
    let status = git::repo::status_paths(&repo).expect("status");
    assert!(
        status.unreadable.is_empty(),
        "an ordinary pass must not claim anything was skipped"
    );
    assert_eq!(status.modified, vec![std::path::PathBuf::from("a.txt")]);
}
