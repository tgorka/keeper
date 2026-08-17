//! Regression: gitoxide MUST be able to launch and drive keeper's own
//! long-running filter (DW-140, guarding DW-206).
//!
//! Registering `filter.lfs.process` is what stops a globally-installed git-lfs
//! from answering for keeper's repositories. It also hands gitoxide a
//! subprocess to launch on every content re-read, and DW-206 is the record of
//! what that costs when the launch fails: `gix_filter::driver::State::
//! maybe_launch_process` errors *before* any driver leniency is consulted, so
//! `status` fails hard whatever `filter.lfs.required` says. Nothing commits, the
//! index is never rewritten, the same entry is re-read next pass — 90 430
//! identical log lines in one user's field measurement, unclearable by restart.
//!
//! So the key this branch installs turns "keeper's filter process starts and
//! speaks protocol version 2 correctly" into a precondition of syncing at all.
//! This test is that precondition, exercised the only way it can be: a real
//! process on the other end of a real pipe.

use gix::index::{entry::Flags, entry::Mode, entry::Stat, State};
use keeper_sync::git;
use keeper_sync::lfs::pointer::Pointer;
use keeper_sync::lfs::store::LfsStore;

/// The filter binary a test may point a git config at. Neither shipping binary
/// can be used here: the app is a GUI shell, and the daemon lives in another
/// crate, which `CARGO_BIN_EXE_` cannot name.
const HARNESS: &str = env!("CARGO_BIN_EXE_lfs-filter-harness");

/// The promise is only made to a binary that keeps it.
///
/// `Engine::open` registers `std::env::current_exe()`, and "whatever executable
/// linked the engine" has been wrong before — DW-121 is months of the app binary
/// being registered for two verbs it did not implement. Under a `process` driver
/// that mistake stops being silent and starts being fatal, so the probe is what
/// decides whether the key is written at all.
#[test]
fn the_probe_tells_a_real_filter_from_a_binary_that_is_not_one() {
    assert!(
        keeper_sync::lfs::filter::serves_process(std::path::Path::new(HARNESS)),
        "a binary that serves the protocol must be recognised"
    );

    // The libtest harness running this very test: an executable that links the
    // engine and answers a filter request with its own argument-parser error.
    // Exactly the shape the probe exists to catch.
    let not_a_filter = std::env::current_exe().expect("locate the test binary");
    assert!(
        !keeper_sync::lfs::filter::serves_process(&not_a_filter),
        "a binary that cannot serve it must never be registered as one"
    );

    // And something that is not a program at all does not panic or hang.
    assert!(!keeper_sync::lfs::filter::serves_process(
        std::path::Path::new("/nonexistent/keeper")
    ));
}

#[test]
fn gitoxide_drives_keepers_filter_process_without_failing_status() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    std::process::Command::new("git")
        .args(["init", "-q", "-b", "main"])
        .current_dir(root)
        .status()
        .expect("git init");
    std::fs::write(
        root.join(".gitattributes"),
        "*.bin filter=lfs diff=lfs merge=lfs -text\n",
    )
    .expect("write attributes");

    // Exactly what `enforce_local_config_with_filter` registers, pointed at a
    // binary that really does serve it.
    for (key, value) in [
        (
            "filter.lfs.process",
            format!(
                "\"{HARNESS}\" lfs filter-process --repo \"{}\"",
                root.display()
            ),
        ),
        (
            "filter.lfs.clean",
            format!("\"{HARNESS}\" lfs clean --repo \"{}\" %f", root.display()),
        ),
        (
            "filter.lfs.smudge",
            format!("\"{HARNESS}\" lfs smudge --repo \"{}\" %f", root.display()),
        ),
        ("filter.lfs.required", "false".to_owned()),
    ] {
        std::process::Command::new("git")
            .args(["config", key, &value])
            .current_dir(root)
            .status()
            .expect("git config");
    }

    // A real object in the store, and the worktree file it stands for.
    let payload = vec![4u8; 300_000];
    let store = LfsStore::in_git_dir(root.join(".git"));
    store.ensure_layout().expect("layout");
    let (oid, size) = store
        .insert_streaming(payload.as_slice())
        .expect("insert the object");
    let big = root.join("video.bin");
    std::fs::write(&big, &payload).expect("write the worktree file");

    // The index entry LFS actually produces: the POINTER's blob, the WORKTREE
    // file's stat (AD-46).
    let repo = git::repo::open(root, false).expect("open");
    let pointer = Pointer::new(oid, size);
    let blob = repo
        .write_blob(pointer.render().as_bytes())
        .expect("write blob")
        .detach();
    let metadata = gix::index::fs::Metadata::from_path_no_follow(&big).expect("stat");
    let stat = Stat::from_fs(&metadata).expect("stat convert");
    let mut index = State::new(repo.object_hash());
    index.dangerously_push_entry(stat, blob, Flags::empty(), Mode::FILE, "video.bin".into());
    index.sort_entries();
    let mut file = gix::index::File::from_state(index, repo.index_path());
    file.write(gix::index::write::Options::default())
        .expect("write index");

    // Commit, so HEAD exists and the entry reads as tracked rather than added.
    // `.gitattributes` joins the index here rather than in the hand-built state
    // above: `git add` merges into the index just written, where rebuilding the
    // state by hand would drop the pointer entry that is the point of the test.
    for args in [
        &["add", ".gitattributes"][..],
        &[
            "-c",
            "user.email=t@e",
            "-c",
            "user.name=t",
            "commit",
            "-q",
            "-m",
            "pointer",
        ][..],
    ] {
        std::process::Command::new("git")
            .args(args)
            .current_dir(root)
            .status()
            .expect("git");
    }

    // Touch the file so its stat no longer matches the entry. That is what sends
    // gix down the *content* comparison — and therefore through the filter —
    // rather than letting the stat shortcut answer. It is the exact trigger
    // DW-206 identifies, and without it this test would pass with no filter
    // configured at all.
    std::fs::write(&big, &payload).expect("re-write to move the stat");

    let repo = git::repo::open(root, false).expect("reopen");
    let status = git::repo::status_paths(&repo)
        .expect("status must not fail: a filter gix cannot drive is DW-206 all over again");
    println!("status with keeper's process filter registered -> {status:?}");

    // The content comparison ran the clean filter, which turned the worktree's
    // 300 KB back into the very pointer the index holds — so the path is clean.
    // A filter that failed, or one that answered with anything else, reports it
    // as modified and the folder never stops re-reading it.
    assert!(
        status.modified.is_empty(),
        "the re-read went through keeper's filter and matched: {status:?}"
    );

    // And the git binary agrees, driving the same registered process.
    let out = std::process::Command::new("git")
        .args(["status", "--short"])
        .current_dir(root)
        .output()
        .expect("git status");
    assert!(
        String::from_utf8_lossy(&out.stdout).trim().is_empty(),
        "git's own view: [{}]",
        String::from_utf8_lossy(&out.stdout).trim()
    );
}
