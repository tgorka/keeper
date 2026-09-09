//! A control file is never a pointer, from any door (Story 70.4, AD-230).
//!
//! Every case here drives the commit path the engine takes — `stage::prepare`
//! then `git::commit::stage_and_commit`, in that order, against a real
//! repository — and reads the answer back out of `HEAD` with the real `git`,
//! because the defect these pin was never in one function: the size rule
//! refused a `.gitattributes` while the attribute rule routed it, and the
//! repository ended up with pointer text where git expected rules. hesperia
//! paid 1 314 669 gitoxide warnings for that on 2026-08-27/28.
//!
//! The attribute door only opens for a path the index already carries, so the
//! fixtures commit each control file as plain text first — through `git`
//! itself, the way history on a real folder came to hold them — and then
//! modify it and commit through keeper.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use keeper_sync::git;
use keeper_sync::lfs::pointer::Pointer;
use keeper_sync::lfs::stage::{self, LfsPolicy};
use keeper_sync::lfs::store::LfsStore;
use keeper_sync::lfs::virtual_policy::{VirtualPolicy, VIRTUAL_PATTERN_FILE};
use keeper_sync::profile::SyncProfile;
use keeper_sync::provenance::{Provenance, SyncSource};

const ATTRIBUTE_SUFFIX: &str = "filter=lfs diff=lfs merge=lfs -text";

fn git(root: &Path, args: &[&str]) {
    let status = std::process::Command::new("git")
        .args(args)
        .current_dir(root)
        .status()
        .expect("git");
    assert!(status.success(), "git {args:?}");
}

fn init_repo(root: &Path) {
    git(root, &["init", "-q", "-b", "main"]);
}

/// Commit `paths` as their raw bytes with the LFS filter bypassed — what an
/// older keeper, or a hand-run `git add`, left in history.
fn commit_raw(root: &Path, paths: &[&str]) {
    let mut add = vec![
        "-c",
        "filter.lfs.process=",
        "-c",
        "filter.lfs.clean=cat",
        "-c",
        "filter.lfs.required=false",
        "add",
        "--",
    ];
    add.extend_from_slice(paths);
    git(root, &add);
    git(
        root,
        &[
            "-c",
            "user.email=t@e",
            "-c",
            "user.name=t",
            "commit",
            "-qm",
            "raw",
        ],
    );
}

fn profile(root: &Path) -> SyncProfile {
    let mut p = SyncProfile::new("01JCTRL", "media", root, "https://git.invalid/r.git");
    p.lfs_threshold_bytes = 1024;
    p
}

/// The engine's own two steps, in the engine's order: route through LFS, then
/// commit with the substitutions routing produced.
fn commit(root: &Path, profile: &SyncProfile, modified: &[&str]) {
    let repo = git::repo::open(root, false).expect("open");
    let store = LfsStore::in_git_dir(root.join(".git"));
    let candidates: Vec<PathBuf> = modified.iter().map(PathBuf::from).collect();
    let staging = stage::prepare(&repo, profile, &store, &candidates).expect("prepare");
    let mut changes = git::commit::StagedChange {
        modified: candidates,
        ..Default::default()
    };
    // What `Engine::commit` does when routing rewrote `.gitattributes`: the
    // rule change lands in the same commit as the files it governs.
    let attributes = PathBuf::from(".gitattributes");
    if staging.attributes_changed && !changes.modified.contains(&attributes) {
        changes.modified.push(attributes);
    }
    let prov = Provenance::new("media", "dev", "01JDEV", "host", SyncSource::Cli);
    let sig = gix::actor::Signature {
        name: "t".into(),
        email: "t@example.invalid".into(),
        time: gix::date::Time::new(1_700_000_000, 0),
    };
    git::commit::stage_and_commit(
        &repo,
        &changes,
        &prov,
        profile,
        &sig,
        &staging.substitutions,
        None,
    )
    .expect("commit")
    .expect("something to commit");
}

/// The bytes `HEAD` records for `path`, read by the real `git`.
fn committed(root: &Path, path: &str) -> Vec<u8> {
    let out = std::process::Command::new("git")
        .args(["show", &format!("HEAD:{path}")])
        .current_dir(root)
        .output()
        .expect("git show");
    assert!(
        out.status.success(),
        "git show HEAD:{path}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    out.stdout
}

/// Legal attribute rules of the requested weight: a file git can read as what
/// it is, on either side of the 1 KiB threshold.
fn rules_text(lines: usize) -> String {
    "*.wav binary\n".repeat(lines)
}

/// The exact defect, reproduced the way the field produced it: a stale
/// anchored rule for a tracked `.gitattributes` below the marker — hesperia's
/// lines 114–115 — and a commit of that path. The size rule already refused
/// it; the attribute rule did not, and `ensure_attributes` only ever appended.
#[test]
fn a_stale_anchored_rule_for_a_control_file_is_retired_and_the_file_is_committed_as_text() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    init_repo(root);
    let p = profile(root);
    let policy = LfsPolicy::from_profile(&p).expect("policy");

    // History: the nested attribute file, tracked as the text it is.
    std::fs::create_dir_all(root.join("a/b")).expect("mkdir");
    std::fs::write(root.join("a/b/.gitattributes"), rules_text(4)).expect("write");
    commit_raw(root, &["a/b/.gitattributes"]);

    // The user's own lines, then keeper's marker and one healthy rule, then
    // the stale one — spelled literally because that is what is on disk.
    let user_lines = "*.txt text\n# mine, do not touch\n";
    std::fs::write(root.join(".gitattributes"), user_lines).expect("seed");
    stage::ensure_attributes(root, &["*.mp4".into()], &policy).expect("marker");
    let mut seed = std::fs::read_to_string(root.join(".gitattributes")).expect("read");
    seed.push_str(&format!("/a/b/.gitattributes {ATTRIBUTE_SUFFIX}\n"));
    std::fs::write(root.join(".gitattributes"), &seed).expect("stale rule");
    commit_raw(root, &[".gitattributes"]);

    // The edit: still small, so only the attribute door could route it.
    std::fs::write(root.join("a/b/.gitattributes"), rules_text(8)).expect("edit");
    commit(root, &p, &["a/b/.gitattributes"]);

    assert_eq!(
        String::from_utf8(committed(root, "a/b/.gitattributes")).expect("utf8"),
        rules_text(8),
        "the committed blob is the file's own rules, not a pointer"
    );
    let after = std::fs::read_to_string(root.join(".gitattributes")).expect("read");
    assert!(
        after.starts_with(user_lines),
        "the user's lines are byte-identical:\n{after}"
    );
    assert!(
        !after.contains("/a/b/.gitattributes"),
        "the stale rule is gone:\n{after}"
    );
    assert!(
        after.contains(&format!("*.mp4 {ATTRIBUTE_SUFFIX}")),
        "the healthy rule stays:\n{after}"
    );
    assert_eq!(
        String::from_utf8(committed(root, ".gitattributes")).expect("utf8"),
        after,
        "and the retirement landed in the same commit"
    );
}

/// F-LFS-2 / F-scan-13. `.lfsconfig` and `.keepervirtual` were protected from
/// virtualization and not from conversion; `.keeper/keeper.toml` carries the
/// `toml` extension, so one oversized TOML anywhere routes it by attribute
/// without it ever crossing the threshold itself. Two of the three are also
/// over the threshold, so both doors are asked and both must refuse; the
/// folder config stays small, so only the attribute door could take it.
#[test]
fn keepers_own_control_files_stay_blobs_though_a_rule_and_the_threshold_both_say_pointer() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    init_repo(root);
    let p = profile(root);
    let policy = LfsPolicy::from_profile(&p).expect("policy");
    stage::ensure_attributes(root, &["*.toml".into()], &policy).expect("toml rule");

    let small = "# a folder config line\n".repeat(8);
    let big = "# a comment line that repeats\n".repeat(80);
    assert!(small.len() < 1024 && big.len() > 1024);
    std::fs::create_dir_all(root.join(".keeper")).expect("mkdir");
    let paths = [
        ".keeper/keeper.toml",
        ".lfsconfig",
        VIRTUAL_PATTERN_FILE,
        "settings.toml",
    ];
    for path in paths {
        std::fs::write(root.join(path), "# v1\n").expect("write");
    }
    commit_raw(
        root,
        &[
            ".gitattributes",
            ".keeper/keeper.toml",
            ".lfsconfig",
            VIRTUAL_PATTERN_FILE,
            "settings.toml",
        ],
    );

    std::fs::write(root.join(".keeper/keeper.toml"), &small).expect("write");
    std::fs::write(root.join(".lfsconfig"), &big).expect("write");
    std::fs::write(root.join(VIRTUAL_PATTERN_FILE), &big).expect("write");
    // The control group: a TOML the rule legitimately governs, under the
    // threshold, so it is the attribute door that routes it.
    std::fs::write(root.join("settings.toml"), &small).expect("write");
    commit(root, &p, &paths);

    assert_eq!(
        committed(root, ".keeper/keeper.toml"),
        small.as_bytes(),
        ".keeper/keeper.toml must be committed as its own bytes"
    );
    for path in [".lfsconfig", VIRTUAL_PATTERN_FILE] {
        assert_eq!(
            committed(root, path),
            big.as_bytes(),
            "{path} must be committed as its own bytes"
        );
    }
    assert!(
        Pointer::parse(&committed(root, "settings.toml")).is_some(),
        "an ordinary TOML under the rule is still routed by attribute"
    );
    let attributes = std::fs::read_to_string(root.join(".gitattributes")).expect("read");
    assert!(
        attributes.contains(&format!("*.toml {ATTRIBUTE_SUFFIX}")),
        "the extension rule is not retired: it governs the rest of the tree\n{attributes}"
    );
}

/// The repair sweep is the third door: an index entry whose attributes say
/// `filter=lfs` and whose blob is raw bytes is what it converts, and a control
/// file under a stale rule is exactly that shape.
#[test]
fn the_repair_sweep_never_offers_a_control_file() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    init_repo(root);
    std::fs::write(
        root.join(".gitattributes"),
        format!("/a/.gitattributes {ATTRIBUTE_SUFFIX}\n*.gif {ATTRIBUTE_SUFFIX}\n"),
    )
    .expect("rules");
    std::fs::create_dir_all(root.join("a")).expect("mkdir");
    std::fs::write(root.join("a/.gitattributes"), "*.wav binary\n").expect("write");
    std::fs::write(root.join("wrong.gif"), vec![9u8; 4096]).expect("write");
    commit_raw(root, &[".gitattributes", "a/.gitattributes", "wrong.gif"]);

    let repo = git::repo::open(root, false).expect("open");
    let tracked = git::repo::tracked_paths(&repo).expect("tracked");
    assert_eq!(
        stage::mismatched_filtered_paths(&repo, &tracked, 0, 100, 100, &HashSet::new()).0,
        vec![PathBuf::from("wrong.gif")],
        "only the gif; the control file is never a repair candidate"
    );
    assert_eq!(
        stage::unconverted_after_repair(&repo, &tracked),
        vec![PathBuf::from("wrong.gif")],
        "and never a failed repair either"
    );
}

/// F-VF-6. Pointer text parses as three legal globs, so a pointerised
/// `.keepervirtual` used to compile into a policy that authorized nothing
/// while `tier()` said one was in force.
#[test]
fn a_policy_file_that_is_pointer_text_is_refused_by_name() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    let file = root.join(VIRTUAL_PATTERN_FILE);
    std::fs::write(&file, Pointer::new("c".repeat(64), 4_000_000).render()).expect("write");

    let err = VirtualPolicy::compile(&profile(root)).expect_err("must refuse");
    assert_eq!(err.code(), "config");
    let text = format!("{err}");
    assert!(
        text.contains(keeper_sync::lfs::virtual_policy::POINTER_TEXT_POLICY_PREFIX),
        "got: {text}"
    );
    assert!(
        text.contains(&file.display().to_string()),
        "names the file: {text}"
    );

    // The ordinary cases are untouched: an empty file is no policy, and a
    // real one compiles.
    std::fs::write(&file, "").expect("empty");
    VirtualPolicy::compile(&profile(root)).expect("empty is silence");
    std::fs::write(&file, "40-media/**\n").expect("rules");
    VirtualPolicy::compile(&profile(root)).expect("a real policy compiles");
}

/// F-scan-14. The detection half: a tracked control file whose worktree bytes
/// are a pointer, named with the subtree it governs.
#[test]
fn a_tracked_control_file_that_is_pointer_text_is_found_with_the_subtree_it_governs() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    init_repo(root);
    std::fs::create_dir_all(root.join("a/b")).expect("mkdir");
    let pointer = Pointer::new("d".repeat(64), 2048).render();
    std::fs::write(root.join("a/b/.gitattributes"), &pointer).expect("write");
    std::fs::write(root.join(".lfsconfig"), &pointer).expect("write");
    std::fs::write(root.join(".gitignore"), "*.tmp\n").expect("write");
    std::fs::write(root.join("note.md"), &pointer).expect("write");
    git(root, &["add", "-A"]);

    let repo = git::repo::open(root, false).expect("open");
    let tracked = git::repo::tracked_paths(&repo).expect("tracked");
    let found = stage::pointerised_control_files(root, &tracked);
    assert_eq!(
        found,
        vec![
            stage::PointerisedControlFile {
                path: PathBuf::from(".lfsconfig"),
                governs: PathBuf::new(),
            },
            stage::PointerisedControlFile {
                path: PathBuf::from("a/b/.gitattributes"),
                governs: PathBuf::from("a/b"),
            },
        ],
        "the healthy .gitignore and the pointer-text note are not findings"
    );
}
