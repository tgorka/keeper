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

/// A path holding a space is ONE pattern, and git agrees (Story 46.1).
///
/// This asks git rather than keeper's own reader, because the two agreeing
/// with each other is exactly what was wrong. Written unquoted,
/// `/2021 holiday/clip filter=lfs …` splits at the blank: git takes `/2021` as
/// the pattern and `holiday/clip` as an attribute NAME to set on it. The rule
/// covers nothing, and it is not a parse error — no exit status and no stderr
/// mark it — which is why the file grew to fifty-nine copies of one rule
/// before anyone noticed.
///
/// The `2021` row is the bug's signature: under the old spelling that path
/// resolved to `lfs`, because `/2021` was what the pattern had become.
#[test]
fn a_path_with_a_space_is_one_pattern_that_git_resolves() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    init_repo(root);

    // Both branches of `pattern_for` can produce a blank: the exact-path one
    // for a name with no extension, and the extension one because
    // `Path::extension` splits at the LAST dot.
    let spaced = stage::pattern_for(Path::new("2021 holiday/clip"));
    let versioned = stage::pattern_for(Path::new("v1.2 final draft"));
    assert_eq!(spaced, "/2021 holiday/clip");
    assert_eq!(versioned, "*.2 final draft");

    stage::ensure_attributes(root, &[spaced, versioned, "*.mp4".into()]).expect("write");

    let (resolved, stderr) = resolved_filters(
        root,
        &[
            "2021 holiday/clip",
            "notes/v1.2 final draft",
            "a.mp4",
            "2021",
        ],
    );
    assert!(
        stderr.is_empty(),
        "git complained about the file keeper wrote:\n{stderr}"
    );
    let filter = |path: &str| resolved.get(path).map(String::as_str);

    assert_eq!(filter("2021 holiday/clip"), Some("lfs"), "{resolved:?}");
    assert_eq!(
        filter("notes/v1.2 final draft"),
        Some("lfs"),
        "{resolved:?}"
    );
    // The quoting must not have cost the ordinary case anything.
    assert_eq!(filter("a.mp4"), Some("lfs"), "{resolved:?}");
    // The prefix the unquoted line used to convert into a rule of its own.
    assert_eq!(filter("2021"), Some("unspecified"), "{resolved:?}");
}

/// What the real `git` binary resolves `filter` to for each path, and what it
/// grumbled about on the way.
///
/// `-z` so a path holding a blank is not re-quoted on the way out and this is
/// not parsing git's own quoting. stderr is returned rather than ignored
/// because it is the only witness that matters for a malformed
/// `.gitattributes` line: git exits 0 and resolves the rule to nothing, so an
/// exit-status assertion passes against the bug.
fn resolved_filters(
    root: &Path,
    paths: &[&str],
) -> (std::collections::BTreeMap<String, String>, String) {
    let mut args = vec!["check-attr", "-z", "filter", "--"];
    args.extend_from_slice(paths);
    let out = std::process::Command::new("git")
        .args(&args)
        .current_dir(root)
        .output()
        .expect("check-attr");
    let text = std::str::from_utf8(&out.stdout).expect("check-attr -z output is utf-8");
    let resolved = text
        .split('\0')
        .collect::<Vec<_>>()
        .as_chunks::<3>()
        .0
        .iter()
        .map(|&[path, _, attr]| (path.to_owned(), attr.to_owned()))
        .collect();
    (resolved, String::from_utf8_lossy(&out.stderr).into_owned())
}

/// DW-194: a glob metacharacter in a real filename is a literal, and git is
/// the witness.
///
/// Quoting cannot reach this layer — unquoting *produces* the pattern and
/// wildmatch interprets it afterwards — so a merely-quoted
/// `"/report [final]"` resolves the real file to `unspecified` and `reportf`
/// to `lfs`, which is exactly backwards. The decoys below are the paths that
/// unescaped character class actually matched.
#[test]
fn a_glob_metacharacter_in_a_real_name_is_a_literal_that_git_resolves() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    init_repo(root);

    // `report [final].pdf` has an extension, so `pattern_for` covers it with
    // `*.pdf` and the bracket never reaches a pattern from that name. The
    // exact-path branch is where it does, and a bracket in a parent directory
    // reaches it alongside a blank — both escapes on one line. All three are
    // asserted, because the acceptance the owner sees is the file resolving.
    let with_extension = stage::pattern_for(Path::new("report [final].pdf"));
    let exact = stage::pattern_for(Path::new("report [final]"));
    let in_a_parent = stage::pattern_for(Path::new("2021 [q4]/clip"));
    assert_eq!(with_extension, "*.pdf");
    assert_eq!(exact, "/report \\[final]");
    assert_eq!(in_a_parent, "/2021 \\[q4]/clip");

    stage::ensure_attributes(root, &[with_extension, exact, in_a_parent]).expect("write");

    let (resolved, stderr) = resolved_filters(
        root,
        &[
            "report [final].pdf",
            "report [final]",
            "2021 [q4]/clip",
            "reportf",
            "reporti",
            "2021 q/clip",
            "2021 4/clip",
        ],
    );
    assert!(
        stderr.is_empty(),
        "git complained about the file keeper wrote:\n{stderr}"
    );
    let filter = |path: &str| resolved.get(path).map(String::as_str);

    assert_eq!(filter("report [final].pdf"), Some("lfs"), "{resolved:?}");
    assert_eq!(filter("report [final]"), Some("lfs"), "{resolved:?}");
    assert_eq!(filter("2021 [q4]/clip"), Some("lfs"), "{resolved:?}");
    // Everything the live character class used to swallow instead.
    for decoy in ["reportf", "reporti", "2021 q/clip", "2021 4/clip"] {
        assert_eq!(filter(decoy), Some("unspecified"), "{resolved:?}");
    }
}

/// DW-193: the lines already on disk are repaired, and git goes quiet.
///
/// The owner's file, reproduced: fifty-nine copies of one rule the pre-46.1
/// writer emitted unquoted, because the reader of the day tokenised its own
/// output the same way the bug did and so never recognised it. git does not
/// fail on them — it exits 0 — it prints `holiday/clip is not a valid
/// attribute name` on stderr for every operation and resolves the rule to
/// nothing. Both halves are asserted, before and after.
#[test]
fn the_broken_lines_already_on_disk_are_repaired_and_git_stops_complaining() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    init_repo(root);

    // Let keeper write its own marker rather than duplicating the constant
    // here; the fixture this test is about is the bytes BELOW it, which are
    // spelled out literally because that is what is on the owner's disk.
    stage::ensure_attributes(root, &["*.mp4".into()]).expect("marker");
    let mut seed = std::fs::read_to_string(root.join(".gitattributes")).expect("read");
    for _ in 0..59 {
        seed.push_str("/2021 holiday/clip filter=lfs diff=lfs merge=lfs -text\n");
    }
    std::fs::write(root.join(".gitattributes"), &seed).expect("seed");

    let (before, complaint) = resolved_filters(root, &["2021 holiday/clip", "2021"]);
    assert!(
        complaint.contains("is not a valid attribute name"),
        "the fixture must actually reproduce the defect, got:\n{complaint}"
    );
    assert_eq!(
        before.get("2021 holiday/clip").map(String::as_str),
        Some("unspecified"),
        "the rule those fifty-nine lines were meant to express does not work"
    );
    assert_eq!(
        before.get("2021").map(String::as_str),
        Some("unspecified"),
        "an INVALID attribute name voids the whole line: unlike the `/a b` case \
         in Story 46.1's matrix, where `b` is a legal name and really is set on \
         `/a`, `holiday/clip` holds a slash, so git discards the rule entirely \
         and even the prefix it silently became gets nothing"
    );

    let pattern = stage::pattern_for(Path::new("2021 holiday/clip"));
    assert!(stage::ensure_attributes(root, std::slice::from_ref(&pattern)).expect("repair"));

    let after = std::fs::read_to_string(root.join(".gitattributes")).expect("read");
    assert_eq!(
        after.matches("2021 holiday/clip").count(),
        1,
        "fifty-nine copies collapse to one:\n{after}"
    );
    assert!(
        after.contains("*.mp4 filter=lfs"),
        "the correct line above them survives:\n{after}"
    );

    let (resolved, stderr) = resolved_filters(root, &["2021 holiday/clip", "2021", "a.mp4"]);
    assert!(
        stderr.is_empty(),
        "git must have nothing left to complain about:\n{stderr}"
    );
    let filter = |path: &str| resolved.get(path).map(String::as_str);
    assert_eq!(filter("2021 holiday/clip"), Some("lfs"), "{resolved:?}");
    assert_eq!(filter("2021"), Some("unspecified"), "{resolved:?}");
    assert_eq!(filter("a.mp4"), Some("lfs"), "{resolved:?}");

    // FR-137: the repaired line is coverage, so the next session writes
    // nothing at all.
    assert!(!stage::ensure_attributes(root, std::slice::from_ref(&pattern)).expect("second"));
    assert_eq!(
        std::fs::read_to_string(root.join(".gitattributes")).expect("read"),
        after
    );
}

/// The same rule twice is one line, and the second run writes nothing.
///
/// FR-137 allows one `.gitattributes` write per session. The writer fix alone
/// would have broken that harder than the bug did: a quoted line tokenises to
/// `"/2021` under a blank-splitting reader, so it never matches and is
/// appended on every single run. Reader and writer are one change, and this is
/// the assertion that says so.
#[test]
fn re_running_ensure_attributes_for_a_spaced_path_leaves_the_file_untouched() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    init_repo(root);

    let patterns = [stage::pattern_for(Path::new("2021 holiday/clip"))];
    assert!(stage::ensure_attributes(root, &patterns).expect("first"));
    let after_first = std::fs::read_to_string(root.join(".gitattributes")).expect("read");

    assert!(
        !stage::ensure_attributes(root, &patterns).expect("second"),
        "the rule keeper wrote a moment ago must read as already present"
    );
    assert!(
        !stage::ensure_attributes(root, &patterns).expect("third"),
        "and it must keep reading that way"
    );
    assert_eq!(
        after_first,
        std::fs::read_to_string(root.join(".gitattributes")).expect("read"),
        "one path is one line, however many times it is staged"
    );
    assert_eq!(after_first.matches("filter=lfs").count(), 1);
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

/// The guard's decision, asked of the guard.
///
/// This deliberately does **not** go through `status_paths`. The state git
/// hands the guard is a racily-clean entry, and reaching that state end to end
/// means making git re-read the worktree — a content comparison that runs
/// `filter.lfs.clean`, which under `cargo test` is the broken filter of DW-121,
/// and whose outcome then differs by platform. `is_false_modification` itself
/// asks none of that: its inputs are the entry's stat, the staged blob, `HEAD`
/// and one `lstat`. Constructing those directly covers every answer
/// deterministically, on every platform, with no clock and no subprocess.
#[test]
fn the_guard_dismisses_an_untouched_pointer_entry_and_nothing_else() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    let payload = vec![42u8; 200_000];
    commit_pointer_for_clip(root, &payload);
    let absolute = root.join("clip.mp4");
    let repo = git::repo::open(root, false).expect("open");

    // Nothing has touched the file since it was staged, so a report about it
    // can only be the re-read finding the pointer design.
    assert!(
        stage::is_false_modification(&repo, Path::new("clip.mp4"), &absolute),
        "an untouched pointer entry is not a modification"
    );
    // A path git has never heard of is not dismissed either.
    assert!(!stage::is_false_modification(
        &repo,
        Path::new("absent.bin"),
        &root.join("absent.bin")
    ));

    // A real edit that keeps the byte count exactly. Comparing the worktree's
    // length against the pointer's `size` — what the guard used to do — cannot
    // see this one, and the user's work would never be committed again.
    let mut edited = payload.clone();
    edited[0] = 7;
    std::fs::write(&absolute, &edited).expect("edit");
    advance_mtime(&absolute);
    let repo = git::repo::open(root, false).expect("reopen");
    assert!(
        stage::indexed_pointer(&repo, Path::new("clip.mp4")).is_some(),
        "the index is untouched, so the path is still LFS-tracked"
    );
    assert!(
        !stage::is_false_modification(&repo, Path::new("clip.mp4"), &absolute),
        "an edit at identical length is still an edit"
    );
    // And git agrees it is one. This report needs no re-read: the stat moved,
    // so the comparison never reaches the racy branch — it holds on every
    // platform, which is why it survives here while the racily-clean half
    // could not.
    assert!(
        git::repo::status_paths(&repo)
            .expect("status")
            .modified
            .contains(&PathBuf::from("clip.mp4")),
        "a same-length edit is reported, so the guard's answer is what decides"
    );

    // An edit that changes the length, and then the file vanishing: neither is
    // the untouched case, and a missing file must never be dismissed.
    std::fs::write(&absolute, vec![7u8; 199_999]).expect("shrink");
    assert!(!stage::is_false_modification(
        &repo,
        Path::new("clip.mp4"),
        &absolute
    ));
    std::fs::remove_file(&absolute).expect("remove");
    assert!(!stage::is_false_modification(
        &repo,
        Path::new("clip.mp4"),
        &absolute
    ));
}

/// An ordinary blob is never dismissed, however still its stat is.
///
/// For a non-pointer entry a re-read that finds different bytes is a real edit
/// caught inside the race window, and swallowing it would lose the user's work.
/// The pointer check is what makes dismissing safe, so it has to be the thing
/// that refuses here — not the stat, which matches.
#[test]
fn the_guard_never_dismisses_an_ordinary_blob() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    init_repo(root);
    std::fs::write(root.join("note.txt"), b"small").expect("write");
    let repo = git::repo::open(root, false).expect("open");
    let changes = git::commit::StagedChange {
        added: vec![PathBuf::from("note.txt")],
        ..Default::default()
    };
    commit(&repo, &changes, &std::collections::BTreeMap::new());

    let repo = git::repo::open(root, false).expect("reopen");
    let absolute = root.join("note.txt");
    assert!(
        stage::indexed_pointer(&repo, Path::new("note.txt")).is_none(),
        "the fixture has to be a non-pointer blob for this to mean anything"
    );
    assert!(
        !stage::is_false_modification(&repo, Path::new("note.txt"), &absolute),
        "an ordinary blob is not the pointer design and must never be dismissed"
    );
}

/// A pointer with no commit behind it is not dismissed.
///
/// The index carries the entry and the worktree matches it, so only `HEAD`
/// can refuse — and on an unborn branch there is no tree to ask. Erring
/// towards "this is real work" is what keeps the path staged.
#[test]
fn the_guard_never_dismisses_a_pointer_with_no_commit_behind_it() {
    use gix::index::{entry::Flags, entry::Mode, entry::Stat, State};

    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    init_repo(root);
    let payload = vec![42u8; 200_000];
    let absolute = root.join("clip.mp4");
    std::fs::write(&absolute, &payload).expect("write");

    let store = LfsStore::in_git_dir(root.join(".git"));
    let staging =
        stage::prepare(&profile(root), &store, &[PathBuf::from("clip.mp4")]).expect("prepare");
    let repo = git::repo::open(root, false).expect("open");
    let blob = repo
        .write_blob(&staging.substitutions[Path::new("clip.mp4")])
        .expect("write blob")
        .detach();

    // The index entry a commit would have written, without the commit.
    let metadata = gix::index::fs::Metadata::from_path_no_follow(&absolute).expect("lstat");
    let stat = Stat::from_fs(&metadata).expect("stat convert");
    let mut state = State::new(repo.object_hash());
    state.dangerously_push_entry(stat, blob, Flags::empty(), Mode::FILE, "clip.mp4".into());
    state.sort_entries();
    gix::index::File::from_state(state, repo.index_path())
        .write(gix::index::write::Options::default())
        .expect("write index");

    let repo = git::repo::open(root, false).expect("reopen");
    assert_eq!(
        stage::indexed_pointer(&repo, Path::new("clip.mp4")),
        Some(staging.uploads[0].oid.clone())
            .map(|oid| keeper_sync::lfs::pointer::Pointer::new(oid, payload.len() as u64)),
        "the staged blob is a pointer, so recognition is not what refuses"
    );
    assert!(
        !stage::is_false_modification(&repo, Path::new("clip.mp4"), &absolute),
        "nothing is committed yet, so the pointer still has to be staged"
    );
}

/// A pointer staged ahead of `HEAD` is not a false modification.
///
/// `stage_and_commit` writes the index before the commit so a crash between
/// the two is re-driven on the next pass (NFR-24). That re-drive only happens
/// because the path is reported modified, so dismissing it would strand the
/// staged pointer until the file changed again.
///
/// This one deliberately does not arrange raciness. The report it relies on is
/// the `HEAD`-to-index difference, which git derives from objects alone, so it
/// holds on every filesystem and whatever the clock does.
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

/// The blocking case from the Story 34.13 review: the guard must discriminate
/// on the path's ROUTING, not on the blob's SHAPE.
///
/// A tracked TEXT file whose content is literally an LFS pointer is an
/// everyday object — documentation about the format, a fixture, a pointer a
/// peer committed with no smudge filter running. The guard first shipped
/// asking `pointer_blob(...).is_some()`, "do these bytes parse as a pointer",
/// which says yes to every one of them. A real edit landing inside git's race
/// window at the same byte count was therefore dismissed — and dismissed again
/// on every later scan, because git keeps reporting it. The user's work is
/// then lost permanently, which is the same outcome the story's Design Note
/// calls data-loss-class when arguing why the old length comparison had to go.
///
/// The state built here is exactly what git hands the guard inside that
/// window: the entry's stat matches the file on disk, the staged blob parses
/// as a pointer, `HEAD` records that blob, and the worktree bytes differ. Each
/// of those is asserted, so nothing but `.gitattributes` routing is left to
/// refuse — and the genuinely routed path in the same repository is dismissed
/// in the same breath, so this is discrimination rather than a blanket
/// refusal.
#[test]
fn an_ordinary_text_file_whose_blob_parses_as_a_pointer_is_never_dismissed() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    let payload = vec![42u8; 200_000];
    // A repository that genuinely uses LFS: `.gitattributes` routes `*.mp4`
    // and `clip.mp4` is a committed pointer.
    commit_pointer_for_clip(root, &payload);

    // ...alongside an ordinary tracked text file that merely *contains* one.
    let doc = root.join("example-pointer.txt");
    let text = Pointer::new("a".repeat(64), 12_345).render();
    std::fs::write(&doc, text.as_bytes()).expect("write");
    let repo = git::repo::open(root, false).expect("open");
    let changes = git::commit::StagedChange {
        added: vec![PathBuf::from("example-pointer.txt")],
        ..Default::default()
    };
    commit(&repo, &changes, &std::collections::BTreeMap::new());

    // The user corrects the oid the document quotes, which preserves the byte
    // count. The entry is then re-stat'ed to what the file now is: that is
    // precisely the racily-clean state, where git re-read the worktree despite
    // the stat matching and found different bytes.
    let edited = Pointer::new("b".repeat(64), 12_345).render();
    assert_eq!(
        edited.len(),
        text.len(),
        "the edit has to preserve the byte count, or the race window is not the subject"
    );
    std::fs::write(&doc, edited.as_bytes()).expect("edit");
    let metadata = gix::index::fs::Metadata::from_path_no_follow(&doc).expect("lstat");
    // Reopened so the index read below is the one the commit just wrote, not a
    // snapshot this handle took before it.
    let repo = git::repo::open(root, false).expect("reopen");
    let shared = repo.index_or_empty().expect("index");
    let mut index: gix::index::File = (**shared).clone();
    index
        .entry_mut_by_path_and_stage(
            gix::bstr::BStr::new("example-pointer.txt"),
            gix::index::entry::Stage::Unconflicted,
        )
        .expect("the document is indexed")
        .stat = gix::index::entry::Stat::from_fs(&metadata).expect("stat convert");
    index
        .write(gix::index::write::Options::default())
        .expect("write index");

    let repo = git::repo::open(root, false).expect("reopen");
    let index = repo.index_or_empty().expect("index");
    let entry = index
        .entry_by_path(gix::bstr::BStr::new("example-pointer.txt"))
        .expect("the document is indexed");
    let after = gix::index::fs::Metadata::from_path_no_follow(&doc).expect("lstat");
    let worktree = gix::index::entry::Stat::from_fs(&after).expect("stat convert");
    assert!(
        worktree.matches(&entry.stat, repo.stat_options().expect("stat options")),
        "the stat has to match, or the stat check would be what refuses and this \
         test would prove nothing"
    );
    assert!(
        stage::indexed_pointer(&repo, Path::new("example-pointer.txt")).is_some(),
        "the blob has to parse as a pointer, or there is no defect to reproduce"
    );
    assert_ne!(
        std::fs::read(&doc).expect("read"),
        text.as_bytes(),
        "the worktree has to hold the edit, or there is nothing to lose"
    );

    assert!(
        !stage::is_false_modification(&repo, Path::new("example-pointer.txt"), &doc),
        "an ordinary text file that merely looks like a pointer is not the pointer \
         design, and dismissing its edit loses it on this scan and every later one"
    );
    assert!(
        stage::is_false_modification(&repo, Path::new("clip.mp4"), &root.join("clip.mp4")),
        "the LFS path the story was written for must still be dismissed"
    );
}

/// The `.gitattributes` rule is what makes a dismissal legitimate, so removing
/// it has to end the dismissal.
///
/// Nothing else moves between the two halves: the same file, the same stat,
/// the same pointer blob, the same `HEAD`. Only the routing changes, which is
/// what makes this the test that fails if `routed_through_lfs` is ever
/// short-circuited to "yes".
#[test]
fn only_the_gitattributes_rule_makes_an_untouched_pointer_dismissible() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    let payload = vec![42u8; 200_000];
    commit_pointer_for_clip(root, &payload);
    let absolute = root.join("clip.mp4");

    let repo = git::repo::open(root, false).expect("open");
    assert!(
        stage::is_false_modification(&repo, Path::new("clip.mp4"), &absolute),
        "a genuinely routed, untouched pointer entry is dismissed — that is the story"
    );

    // The user stops routing `*.mp4` through LFS. The rule is rewritten rather
    // than the file deleted on purpose: `Source::WorktreeThenIdMapping` falls
    // back to the indexed `.gitattributes` when the worktree has none, and the
    // committed copy still carries the rule.
    let attributes = std::fs::read_to_string(root.join(".gitattributes")).expect("read");
    assert!(
        attributes.contains("*.mp4 filter=lfs"),
        "the fixture writes the rule this test removes"
    );
    let without: String = attributes
        .lines()
        .filter(|line| !line.starts_with("*.mp4 "))
        .map(|line| format!("{line}\n"))
        .collect();
    std::fs::write(root.join(".gitattributes"), without).expect("rewrite");

    let repo = git::repo::open(root, false).expect("reopen");
    assert!(
        stage::indexed_pointer(&repo, Path::new("clip.mp4")).is_some(),
        "the blob is untouched, so the blob's shape is not what changed"
    );
    assert!(
        !stage::is_false_modification(&repo, Path::new("clip.mp4"), &absolute),
        "with nothing routing it through LFS the path is ordinary, and an ordinary \
         path's racily-clean report is a real edit"
    );
}

/// An empty tracked file is not an LFS entry (Story 34.13 review, major).
///
/// `Pointer::parse` reads zero bytes as the *empty pointer* — the LFS spec's
/// carve-out for empty files, and correct where it is, since `lfs::filter`
/// depends on it. Inheriting it in `pointer_blob` was not: every empty tracked
/// file in a repository shares the one empty blob, so `indexed_pointer`
/// answered `Some` for all of them and the guard held a dismissible entry for
/// each — one value away from the I/O matrix's "not a pointer | `None`".
///
/// Everything here is routed through LFS by a `*` rule so that routing cannot
/// be what refuses; only the blob's emptiness can be.
#[test]
fn an_empty_tracked_file_is_not_an_lfs_entry() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    init_repo(root);
    std::fs::write(
        root.join(".gitattributes"),
        "* filter=lfs diff=lfs merge=lfs -text\n",
    )
    .expect("write attributes");
    let absolute = root.join("empty.txt");
    std::fs::write(&absolute, b"").expect("write");

    let repo = git::repo::open(root, false).expect("open");
    let changes = git::commit::StagedChange {
        added: vec![PathBuf::from(".gitattributes"), PathBuf::from("empty.txt")],
        ..Default::default()
    };
    commit(&repo, &changes, &std::collections::BTreeMap::new());

    let repo = git::repo::open(root, false).expect("reopen");
    assert!(
        stage::indexed_pointer(&repo, Path::new("empty.txt")).is_none(),
        "an empty blob is an empty file, not an LFS pointer"
    );
    assert!(
        !stage::is_false_modification(&repo, Path::new("empty.txt"), &absolute),
        "so there is no pointer design behind it for the guard to dismiss"
    );
}

/// Routing that cannot be determined is never read as routed.
///
/// The unknown case has to err towards staging, because the two errors are not
/// symmetric: declining to dismiss costs one re-clean whose identical pointer
/// leaves the tree unchanged and produces no commit, while dismissing wrongly
/// loses the user's edit for good.
///
/// `core.attributesFile` naming a home directory that cannot be resolved makes
/// the attribute stack refuse to assemble at all, which is the one shape of
/// "keeper cannot tell whether this path is routed through LFS" a test can
/// arrange deterministically on both Linux and macOS. The fixture is proven
/// dismissable first, so the refusal afterwards can only come from the answer
/// having gone missing.
#[test]
fn a_path_whose_lfs_routing_cannot_be_determined_is_never_dismissed() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    let payload = vec![42u8; 200_000];
    commit_pointer_for_clip(root, &payload);
    let absolute = root.join("clip.mp4");

    let repo = git::repo::open(root, false).expect("open");
    assert!(
        stage::is_false_modification(&repo, Path::new("clip.mp4"), &absolute),
        "the fixture has to be dismissable, or the refusal below proves nothing"
    );

    let ok = std::process::Command::new("git")
        .args([
            "config",
            "core.attributesFile",
            "~keeper-nobody-34-13/attributes",
        ])
        .current_dir(root)
        .status()
        .expect("git config")
        .success();
    assert!(ok, "git config must succeed");

    // `trust_full` so the repo-local `core.attributesFile` above is certainly
    // honoured: gitoxide only reads repository-kind config keys at full trust
    // (`gix::config::section::is_trusted`), and this test's whole subject is
    // that key being consulted and failing.
    let repo = git::repo::open(root, true).expect("reopen");
    assert!(
        stage::indexed_pointer(&repo, Path::new("clip.mp4")).is_some(),
        "nothing about the blob changed, so the blob's shape is not what refuses"
    );
    assert!(
        !stage::is_false_modification(&repo, Path::new("clip.mp4"), &absolute),
        "an unanswerable routing question must never be read as routed"
    );
}
