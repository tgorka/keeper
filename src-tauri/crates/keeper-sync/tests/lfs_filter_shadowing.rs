//! A `filter "lfs"` driver from outside the repository must not decide how a
//! managed folder is filtered — and must never be able to stop `status`.
//!
//! Field incident, 2026-08-13 → 2026-08-16: one user's log carried 90 430
//! identical lines,
//!
//! ```text
//! sync retrying profile="…" error=git object store failure: status failed: IO
//! error while writing blob or reading file metadata or changing filetype:
//! Process handshake with command GIT_DIR="…" GIT_WORK_TREE="…" "/bin/sh" "-c"
//! "git-lfs filter-process" "sh" failed: Failed to read or write to the process:
//! failed to fill whole buffer
//! ```
//!
//! and the profile never committed again. The cause is the *other half* of the
//! `git lfs install` whose hooks `hooks_never_run.rs` covers: the same command
//! writes a `filter "lfs"` driver into the user's **global** config, gitoxide
//! answers with the FIRST section of that name rather than merging keys across
//! scopes, and global precedes local — so keeper's own `clean`/`smudge` and its
//! `required = false` were never consulted at all. `process` is the long-running
//! protocol, its launch failure is fatal whatever `required` says, and on a
//! desktop launch `PATH` is Finder's, so `git-lfs` is not there to launch.
//!
//! The state is self-perpetuating: `status` fails, so nothing commits, so the
//! index is never rewritten, so the entry keeps asking the filter for content.
//!
//! This has to be an end-to-end probe with a real global configuration file,
//! which means a real environment variable — so the work happens in a child
//! process (the pattern `durability_matrix_stages.rs` uses), because the
//! workspace forbids `unsafe_code` and `std::env::set_var` is `unsafe`.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, SystemTime};

use gix::index::{
    entry::{Flags, Mode, Stat},
    State,
};
use keeper_sync::git;
use keeper_sync::lfs::pointer::Pointer;

/// Switches [`child_probes_a_global_git_lfs_driver`] from a no-op into the real
/// probe. The child is an ordinary `#[test]` in this same binary, re-invoked
/// through `current_exe()`, so it must be inert on a normal run.
const CHILD_ENV: &str = "KEEPER_SYNC_LFS_SHADOW_CHILD";

/// What `git lfs install` writes into `~/.gitconfig`, except that the program is
/// spelled so that no machine can have it.
///
/// The field config says plain `git-lfs`, and the failure there is that it cannot
/// be **launched** — a desktop launch inherits Finder's `PATH`, which has no
/// Homebrew in it. Spelling `git-lfs` here would make the measurement depend on
/// the host: the macOS CI runner ships a working `git-lfs`, its handshake
/// succeeds, and it then cleans the payload back into the very pointer the index
/// holds — so the entry reads CLEAN and nothing fails. That is the *silent* half
/// of the same defect (another installation's driver quietly becomes the filter
/// for a folder keeper manages, storing objects keeper's journal knows nothing
/// about), and it is the second reason the whole section goes rather than only
/// the key that crashes.
const GLOBAL_GIT_LFS_CONFIG: &str = "[filter \"lfs\"]\n\tprocess = keeper-test-no-such-git-lfs filter-process\n\tclean = keeper-test-no-such-git-lfs clean -- %f\n\tsmudge = keeper-test-no-such-git-lfs smudge -- %f\n\trequired = true\n";

#[test]
fn a_global_git_lfs_driver_does_not_break_status() {
    if std::env::var_os(CHILD_ENV).is_some() {
        return;
    }
    let dir = tempfile::tempdir().expect("tempdir");
    let global = dir.path().join("gitconfig");
    std::fs::write(&global, GLOBAL_GIT_LFS_CONFIG).expect("write the global config");

    let exe = std::env::current_exe().expect("locate the current test executable");
    let out = Command::new(&exe)
        .args([
            "--exact",
            "child_probes_a_global_git_lfs_driver",
            "--nocapture",
        ])
        .env(CHILD_ENV, "1")
        // Both global sources resolve through this variable, so the child sees
        // the driver twice — which is also how a machine with a system *and* a
        // global git-lfs installation looks.
        .env("GIT_CONFIG_GLOBAL", &global)
        // The host's own /etc/gitconfig has no business in this measurement.
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .expect("run the child probe");

    assert!(
        out.status.success(),
        "child probe failed ({}):\n{}{}",
        out.status,
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn child_probes_a_global_git_lfs_driver() {
    if std::env::var_os(CHILD_ENV).is_none() {
        return;
    }
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    build_lfs_fixture(root);

    // The reproduction, so a green run cannot be green for the wrong reason: a
    // handle opened the way gitoxide opens one — no surgery — must still fail,
    // or this fixture never reaches the filter and proves nothing.
    let unprotected = gix::open_opts(
        root,
        gix::open::Options::default().with(gix::sec::Trust::Full),
    )
    .expect("open without keeper's surgery");
    let err = git::repo::status_paths(&unprotected)
        .expect_err("a driver that cannot be launched must break this status");
    let message = err.to_string();
    println!("unprotected handle -> {message}");
    assert!(
        message.contains("Process handshake"),
        "the fixture must fail on the filter driver, not on something else: {message}"
    );

    // And the same fixture, through the door the engine uses.
    let repo = git::repo::open(root, true).expect("keeper's open");
    let status = git::repo::status_paths(&repo).expect("status must survive a foreign lfs driver");
    println!("keeper's handle -> {status:?}");
    assert!(
        status.modified.contains(&PathBuf::from("video.bin")),
        "the edited pointer path has to be reported as work to do: {status:?}"
    );
}

/// A repository whose index holds an LFS **pointer** paired with the worktree
/// file's stat (AD-46), edited so that `gix::status` has to read its content.
///
/// The edit moves the file's mtime while leaving its size alone, because
/// `FastEq` streams content only when the sizes agree — that is the one shape
/// that reaches the filter, and it is the shape a recording touched after the
/// last index write has.
fn build_lfs_fixture(root: &Path) {
    let repo = gix::init(root).expect("init");
    std::fs::write(root.join(".gitattributes"), "*.bin filter=lfs -text\n").expect("attributes");

    let payload = vec![7u8; 4096];
    let big = root.join("video.bin");
    std::fs::write(&big, &payload).expect("write the payload");

    // What `Engine::open` registers: this executable, `lfs clean|smudge`, never
    // required. Under a test binary the program is the libtest harness, which
    // answers a filter request with its own argument-parser error — and
    // `required = false` is exactly what makes that recoverable.
    //
    // `false` for the long-running driver, which is what the engine itself
    // passes here: `lfs::filter::serves_process` probes the program and a
    // libtest harness fails that handshake, so no `process` key is written. The
    // fixture depends on it — a `process` driver naming a binary that cannot
    // serve it fails `status` outright rather than recoverably (DW-140).
    let exe = std::env::current_exe().expect("locate the current test executable");
    git::repo::enforce_local_config_with_filter(&repo, Some(&exe), false)
        .expect("register the filter");

    let digest = {
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(&payload);
        hex::encode(hasher.finalize())
    };
    let pointer = Pointer::new(digest, payload.len() as u64);
    let blob = repo
        .write_blob(pointer.render().as_bytes())
        .expect("write the pointer blob")
        .detach();

    let metadata = gix::index::fs::Metadata::from_path_no_follow(&big).expect("stat the payload");
    let stat = Stat::from_fs(&metadata).expect("convert the stat");
    let mut index = State::new(repo.object_hash());
    index.dangerously_push_entry(stat, blob, Flags::empty(), Mode::FILE, "video.bin".into());
    index.sort_entries();
    let mut file = gix::index::File::from_state(index, repo.index_path());
    file.write(gix::index::write::Options::default())
        .expect("write the index");

    let times = std::fs::FileTimes::new()
        .set_modified(SystemTime::now() - Duration::from_secs(3600))
        .set_accessed(SystemTime::now());
    std::fs::File::options()
        .write(true)
        .open(&big)
        .expect("reopen the payload")
        .set_times(times)
        .expect("move the payload's mtime");
}
