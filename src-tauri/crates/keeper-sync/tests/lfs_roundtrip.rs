//! End-to-end LFS behaviour against a real repository (Stories 25.1–25.4).
//!
//! These exercise the whole clean/smudge path the way a user meets it, because
//! the interesting failures here are not in any single function: they are in
//! whether git, gitoxide and keeper all agree about what a tracked file
//! contains. Unit tests could not have caught the two defects that motivated
//! this file — a pointer blob reading as permanently modified under plain
//! `git`, and a materialized object leaving a stale index stat behind.
//!
//! No network: the transfer layer has its own tests, and what matters here is
//! the local object store plus the filter.

use std::path::{Path, PathBuf};

use keeper_sync::git;
use keeper_sync::lfs::pointer::Pointer;
use keeper_sync::lfs::stage;
use keeper_sync::lfs::store::LfsStore;
use keeper_sync::profile::SyncProfile;
use keeper_sync::provenance::{Provenance, SyncSource};

/// The daemon binary doubles as the clean/smudge filter, so the tests point git
/// at whatever test binary is running — it is not the daemon, so the filter
/// tests below drive `stage`/`store` directly and only the *registration* is
/// asserted against git.
fn init_repo(root: &Path) {
    let ok = std::process::Command::new("git")
        .args(["init", "-q", "-b", "main"])
        .current_dir(root)
        .status()
        .expect("git init")
        .success();
    assert!(ok, "git init must succeed");
}

fn profile(root: &Path) -> SyncProfile {
    let mut p = SyncProfile::new("01JTEST", "media", root, "https://git.invalid/r.git");
    p.lfs_threshold_bytes = 1024;
    p
}

fn commit(
    repo: &gix::Repository,
    changes: &git::commit::StagedChange,
    subs: &std::collections::BTreeMap<PathBuf, Vec<u8>>,
) {
    let prov = Provenance::new("media", "dev", "01JDEV", "host", SyncSource::Cli);
    let sig = gix::actor::Signature {
        name: "t".into(),
        email: "t@example.invalid".into(),
        time: gix::date::Time::new(1_700_000_000, 0),
    };
    let root = repo
        .workdir()
        .expect("the fixture repository has a work tree");
    git::commit::stage_and_commit(repo, changes, &prov, &profile(root), &sig, subs, None)
        .expect("commit");
}

#[test]
fn an_oversized_file_is_committed_as_a_pointer_while_the_worktree_keeps_it() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    init_repo(root);

    let payload = vec![42u8; 200_000];
    std::fs::write(root.join("clip.mp4"), &payload).expect("write");
    std::fs::write(root.join("note.txt"), b"small").expect("write");

    let p = profile(root);
    let store = LfsStore::in_git_dir(root.join(".git"));
    let candidates = vec![PathBuf::from("clip.mp4"), PathBuf::from("note.txt")];
    let staging = stage::prepare(&p, &store, &candidates).expect("prepare");

    // Only the oversized file is routed.
    assert_eq!(staging.substitutions.len(), 1);
    assert!(staging.attributes_changed);

    let repo = git::repo::open(root, false).expect("open");
    let mut changes = git::commit::StagedChange {
        added: candidates.clone(),
        ..Default::default()
    };
    changes.added.push(PathBuf::from(".gitattributes"));
    commit(&repo, &changes, &staging.substitutions);

    // git stored a pointer for the big file and the real bytes for the small one.
    let blob = std::process::Command::new("git")
        .args(["cat-file", "-s", "HEAD:clip.mp4"])
        .current_dir(root)
        .output()
        .expect("cat-file");
    let blob_size: u64 = String::from_utf8_lossy(&blob.stdout)
        .trim()
        .parse()
        .expect("size");
    assert!(
        blob_size < 1024,
        "the blob must be a pointer, got {blob_size} bytes"
    );

    // …and the user's file is untouched.
    assert_eq!(std::fs::read(root.join("clip.mp4")).expect("read"), payload);
    // …and the object is in the store, addressable by its digest.
    let oid = &staging.uploads[0].oid;
    assert!(store.contains(oid, payload.len() as u64));
}

#[test]
fn a_checked_out_pointer_is_materialized_and_leaves_the_index_consistent() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    init_repo(root);

    // Seed the store with an object, and put only its pointer in the worktree —
    // exactly the state a fresh checkout of an LFS path leaves behind.
    let payload = vec![5u8; 150_000];
    let store = LfsStore::in_git_dir(root.join(".git"));
    store.ensure_layout().expect("layout");
    let (oid, size) = store
        .insert_streaming(std::io::Cursor::new(payload.clone()))
        .expect("insert");
    let pointer = Pointer::new(oid.clone(), size);
    std::fs::write(root.join("clip.mp4"), pointer.render()).expect("write pointer");

    let repo = git::repo::open(root, false).expect("open");
    let changes = git::commit::StagedChange {
        added: vec![PathBuf::from("clip.mp4")],
        ..Default::default()
    };
    commit(&repo, &changes, &std::collections::BTreeMap::new());

    let tracked = git::repo::tracked_paths(&repo).expect("tracked");
    let pending = stage::pending_smudges(root, &tracked).expect("pending");
    assert_eq!(pending.len(), 1, "the pointer file must be recognised");
    assert_eq!(pending[0].pointer.oid, oid);

    stage::materialize(&store, root, &pending[0]).expect("materialize");
    assert_eq!(
        std::fs::read(root.join("clip.mp4")).expect("read"),
        payload,
        "the worktree must now hold the real content"
    );

    // The index still records the pointer's stat, which would make status call
    // the file modified. Refreshing it is what keeps the invariant.
    git::repo::refresh_index_stat(&repo, &[PathBuf::from("clip.mp4")]).expect("refresh");
    let debug = std::process::Command::new("git")
        .args(["ls-files", "--debug", "clip.mp4"])
        .current_dir(root)
        .output()
        .expect("ls-files");
    let text = String::from_utf8_lossy(&debug.stdout);
    assert!(
        text.contains(&format!("size: {}", payload.len())),
        "the index must record the materialized size, got:\n{text}"
    );
}

#[test]
fn materializing_is_atomic_so_an_absent_object_never_truncates_the_pointer() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    let store = LfsStore::in_git_dir(root.join(".git"));
    store.ensure_layout().expect("layout");

    let pointer = Pointer::new("b".repeat(64), 999);
    std::fs::write(root.join("missing.bin"), pointer.render()).expect("write pointer");
    let smudge = stage::PendingSmudge {
        path: PathBuf::from("missing.bin"),
        pointer: pointer.clone(),
    };

    let err = stage::materialize(&store, root, &smudge).expect_err("must refuse");
    assert_eq!(err.code(), "integrity");
    // The pointer survives intact, so the transfer can simply be retried.
    assert_eq!(
        std::fs::read_to_string(root.join("missing.bin")).expect("read"),
        pointer.render()
    );
}

#[test]
fn attributes_written_for_one_profile_are_readable_as_git_attributes() {
    // The generated file must be something git itself accepts, not merely
    // something we can parse back.
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    init_repo(root);
    stage::ensure_attributes(root, &["*.mp4".into(), "*.iso".into()]).expect("write");

    let out = std::process::Command::new("git")
        .args(["check-attr", "filter", "--", "a.mp4", "b.iso", "c.txt"])
        .current_dir(root)
        .output()
        .expect("check-attr");
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("a.mp4: filter: lfs"), "got:\n{text}");
    assert!(text.contains("b.iso: filter: lfs"), "got:\n{text}");
    assert!(text.contains("c.txt: filter: unspecified"), "got:\n{text}");
}

/// A repository whose `clip.mp4` is committed as an LFS pointer while the
/// worktree keeps the real bytes — the state every LFS-tracked path lives in.
fn commit_pointer_for_clip(root: &Path, payload: &[u8]) -> stage::LfsStaging {
    init_repo(root);
    std::fs::write(root.join("clip.mp4"), payload).expect("write");
    let store = LfsStore::in_git_dir(root.join(".git"));
    let staging =
        stage::prepare(&profile(root), &store, &[PathBuf::from("clip.mp4")]).expect("prepare");
    let repo = git::repo::open(root, false).expect("open");
    let changes = git::commit::StagedChange {
        added: vec![PathBuf::from(".gitattributes"), PathBuf::from("clip.mp4")],
        ..Default::default()
    };
    commit(&repo, &changes, &staging.substitutions);
    staging
}

/// Make every index entry racily clean.
///
/// git re-reads the content of an entry whose mtime is not older than the
/// index file itself. A test cannot wait for that — the index is always
/// written last — so the condition is arranged by moving the index's own mtime
/// back onto the worktree file's.
fn backdate_index_to(root: &Path, file: &Path) {
    let mtime = std::fs::metadata(file)
        .and_then(|metadata| metadata.modified())
        .expect("worktree mtime");
    std::fs::File::options()
        .write(true)
        .open(root.join(".git").join("index"))
        .and_then(|index| index.set_modified(mtime))
        .expect("backdate the index");
}

/// Move a file's mtime a clear second forward.
///
/// Stat comparison uses mtime *seconds* unless `core.checkStat` asks for
/// nanoseconds, so a rewrite inside the same second is indistinguishable to
/// git itself and a test must not depend on how fast it runs.
fn advance_mtime(path: &Path) {
    let when = std::time::SystemTime::now() + std::time::Duration::from_secs(10);
    std::fs::File::options()
        .write(true)
        .open(path)
        .and_then(|file| file.set_modified(when))
        .expect("advance the mtime");
}

/// Register a genuinely working `filter.lfs.clean` for this repository.
///
/// Not a stub that replays a known pointer: it hashes whatever it is given, so
/// it keeps telling the truth after the worktree file changes. `keeper-syncd`'s
/// `lfs clean` (`commands.rs:1621`) is the real article and cannot be run from
/// here — an integration test's `current_exe` is the libtest harness — so the
/// shape is reproduced with the two digest tools that exist on the platforms
/// keeper builds for.
fn register_working_clean_filter(root: &Path) {
    let script = root.join("clean-filter.sh");
    std::fs::write(
        &script,
        "#!/bin/sh\nt=$(mktemp)\ncat > \"$t\"\n\
         h=$(sha256sum \"$t\" 2>/dev/null | cut -d' ' -f1 || shasum -a 256 \"$t\" | cut -d' ' -f1)\n\
         printf 'version https://git-lfs.github.com/spec/v1\\noid sha256:%s\\nsize %s\\n' \\\n\
         \"$h\" \"$(wc -c < \"$t\" | tr -d ' ')\"\nrm -f \"$t\"\n",
    )
    .expect("write filter");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).expect("chmod");
    }
    // The same single-invocation shape `enforce_local_config_with_filter`
    // writes, minus the `--repo` argument this stand-in has no use for.
    for (key, value) in [
        ("filter.lfs.clean", format!("\"{}\"", script.display())),
        ("filter.lfs.required", "false".to_owned()),
    ] {
        let ok = std::process::Command::new("git")
            .args(["config", key, &value])
            .current_dir(root)
            .status()
            .expect("git config")
            .success();
        assert!(ok, "git config {key} must succeed");
    }
}

/// Regression: `indexed_pointer` decided "too big to be a pointer" from
/// `entry.stat.size`, and an LFS entry's stat is the WORKTREE file's — so it
/// answered `None` for every real LFS file, the only thing it exists to
/// recognise. The blob's own length is the question being asked.
#[test]
fn an_indexed_pointer_is_recognised_although_its_entry_stat_is_the_worktree_files() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    let payload = vec![42u8; 200_000];
    let staging = commit_pointer_for_clip(root, &payload);

    // Two ordinary blobs to bracket it: one far too large to be a pointer, one
    // small enough to read but not one either.
    std::fs::write(root.join("prose.txt"), vec![b'x'; 200_000]).expect("write");
    std::fs::write(root.join("note.txt"), b"small").expect("write");
    let repo = git::repo::open(root, false).expect("open");
    let changes = git::commit::StagedChange {
        added: vec![PathBuf::from("note.txt"), PathBuf::from("prose.txt")],
        ..Default::default()
    };
    commit(&repo, &changes, &std::collections::BTreeMap::new());

    let repo = git::repo::open(root, false).expect("reopen");
    // The premise of the bug: the entry's stat measures the worktree file.
    let index = repo.index_or_empty().expect("index");
    let entry = index
        .entry_by_path(gix::bstr::BStr::new("clip.mp4"))
        .expect("clip.mp4 is indexed");
    assert_eq!(
        u64::from(entry.stat.size),
        payload.len() as u64,
        "the entry's stat is the worktree file's, which is what made the old size test wrong"
    );

    let indexed = stage::indexed_pointer(&repo, Path::new("clip.mp4"))
        .expect("the staged blob is a pointer and has to be recognised");
    assert_eq!(indexed.oid, staging.uploads[0].oid);
    assert_eq!(indexed.size, payload.len() as u64);

    assert!(
        stage::indexed_pointer(&repo, Path::new("prose.txt")).is_none(),
        "a 200 000-byte blob cannot be a pointer"
    );
    assert!(
        stage::indexed_pointer(&repo, Path::new("note.txt")).is_none(),
        "a small blob that is not a pointer is still not one"
    );
    assert!(stage::indexed_pointer(&repo, Path::new("absent.bin")).is_none());
}

/// The racily-clean guard, which no LFS path ever reached while
/// `indexed_pointer` was rejecting them all.
#[test]
fn a_racily_clean_pointer_is_not_a_modification_but_an_equal_length_edit_is() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    let payload = vec![42u8; 200_000];
    commit_pointer_for_clip(root, &payload);
    let absolute = root.join("clip.mp4");

    backdate_index_to(root, &absolute);
    let repo = git::repo::open(root, false).expect("reopen");
    let status = git::repo::status_paths(&repo).expect("status");
    assert!(
        status.modified.contains(&PathBuf::from("clip.mp4")),
        "the fixture has to reproduce the racily-clean re-read, got {status:?}"
    );
    assert!(
        stage::is_false_modification(&repo, Path::new("clip.mp4"), &absolute),
        "nothing touched the file, so the re-read found the design and not an edit"
    );

    // A real edit that keeps the byte count exactly. Comparing the worktree's
    // length against the pointer's `size` — what the guard used to do — cannot
    // see it, and the user's work would never be committed again.
    let mut edited = payload.clone();
    edited[0] = 7;
    std::fs::write(&absolute, &edited).expect("edit");
    advance_mtime(&absolute);

    let repo = git::repo::open(root, false).expect("reopen");
    assert!(
        git::repo::status_paths(&repo)
            .expect("status")
            .modified
            .contains(&PathBuf::from("clip.mp4")),
        "git still reports the edit"
    );
    assert!(
        stage::indexed_pointer(&repo, Path::new("clip.mp4")).is_some(),
        "the index is untouched, so the path is still LFS-tracked"
    );
    assert!(
        !stage::is_false_modification(&repo, Path::new("clip.mp4"), &absolute),
        "an edit at identical length is still an edit"
    );
}

/// A pointer staged ahead of `HEAD` is not a false modification.
///
/// `stage_and_commit` writes the index before the commit so a crash between
/// the two is re-driven on the next pass (NFR-24). That re-drive only happens
/// because the path is reported modified, so dismissing it would strand the
/// staged pointer until the file changed again.
#[test]
fn a_pointer_staged_ahead_of_head_is_never_dismissed() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    let payload = vec![42u8; 200_000];
    commit_pointer_for_clip(root, &payload);
    let absolute = root.join("clip.mp4");

    // Stage a different pointer for the same path and stop there, exactly as a
    // crash between the index write and the commit leaves things.
    let repo = git::repo::open(root, false).expect("open");
    let ahead = Pointer::new("c".repeat(64), payload.len() as u64);
    let blob = repo
        .write_blob(ahead.render().as_bytes())
        .expect("write blob")
        .detach();
    let shared = repo.index_or_empty().expect("index");
    let mut index: gix::index::File = (**shared).clone();
    index
        .entry_mut_by_path_and_stage(
            gix::bstr::BStr::new("clip.mp4"),
            gix::index::entry::Stage::Unconflicted,
        )
        .expect("clip.mp4 is indexed")
        .id = blob;
    index
        .write(gix::index::write::Options::default())
        .expect("write index");

    let repo = git::repo::open(root, false).expect("reopen");
    assert!(
        git::repo::status_paths(&repo)
            .expect("status")
            .modified
            .contains(&PathBuf::from("clip.mp4")),
        "the index is ahead of HEAD, so the path is reported modified"
    );
    assert_eq!(
        stage::indexed_pointer(&repo, Path::new("clip.mp4")),
        Some(ahead),
        "the staged blob is still a pointer, so recognition is not what refuses"
    );
    assert!(
        !stage::is_false_modification(&repo, Path::new("clip.mp4"), &absolute),
        "the pointer is staged but not committed, so the next pass has to re-drive it"
    );
}

/// Which configuration the racily-clean guard is actually load-bearing in.
///
/// A working `filter.lfs.clean` answers the question before keeper does: git
/// re-reads the worktree, cleans it back into the pointer it already has, and
/// calls the path unchanged, so `is_false_modification` is never consulted.
/// Without one — `filter.lfs.required` is `false`, so a filter that fails is a
/// filter that is not there — the raw bytes are hashed, the path reads as
/// modified, and the guard is the only thing between the user and a full
/// re-clean of the file on every pass.
///
/// That difference is not academic. `Engine` registers `std::env::current_exe()`
/// as the filter (`engine.rs:342-344`), which is `keeper-syncd` in a CLI run —
/// and the desktop app binary, which has no `lfs` subcommand at all. So the
/// unfiltered column is the desktop configuration, not a test artefact.
#[test]
fn a_working_clean_filter_settles_the_racily_clean_case_before_the_guard_does() {
    let payload = vec![42u8; 200_000];
    let mut edited = payload.clone();
    edited[0] = 7;

    for filtered in [false, true] {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        commit_pointer_for_clip(root, &payload);
        if filtered {
            register_working_clean_filter(root);
        }
        let absolute = root.join("clip.mp4");
        backdate_index_to(root, &absolute);

        let repo = git::repo::open(root, false).expect("open");
        let racily_clean = git::repo::status_paths(&repo).expect("status");
        assert_eq!(
            racily_clean.modified.contains(&PathBuf::from("clip.mp4")),
            !filtered,
            "filtered={filtered}: a working filter resolves the re-read itself, \
             and without one the guard has to; got {racily_clean:?}"
        );

        // A real edit is reported either way, so the guard's other answer is
        // reachable in both configurations.
        std::fs::write(&absolute, &edited).expect("edit");
        advance_mtime(&absolute);
        let repo = git::repo::open(root, false).expect("reopen");
        assert!(
            git::repo::status_paths(&repo)
                .expect("status")
                .modified
                .contains(&PathBuf::from("clip.mp4")),
            "filtered={filtered}: an edit at identical length is reported whatever the filter does"
        );
    }
}
