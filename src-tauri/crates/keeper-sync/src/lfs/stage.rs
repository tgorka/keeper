//! Routing oversized files through LFS at stage time (Stories 25.4, 25.5;
//! AD-46).
//!
//! # Why this exists at all
//!
//! gitoxide has **no streaming object read** — `try_find` always fills a
//! buffer — so a 3 GB file committed as an ordinary blob is a 3 GB allocation
//! every time anything touches it. Above a threshold, content must never
//! become a git blob.
//!
//! # Why there is no filter subprocess
//!
//! The obvious route is registering `filter.lfs.process` so gitoxide's filter
//! pipeline does clean/smudge for us. It is not needed. Verified empirically
//! (`tests/lfs_pointer_stat.rs`): an index entry whose **blob is the pointer**
//! but whose **stat is the worktree file's** reads as clean from both
//! `gix::status` and `git status`. That is precisely how real git+LFS stays
//! fast, and it lets keeper stay a single process with no `%f` quoting, no
//! protocol handshake, and no dependency on a `git-lfs` binary.
//!
//! The one wrinkle is git's *racily clean* rule: an entry whose mtime is not
//! older than the index is re-read regardless of stat. Right after staging,
//! that is every file we just touched, and re-reading finds the worktree bytes
//! differing from the pointer blob. [`is_false_modification`] answers that
//! without hashing gigabytes twice.

use std::borrow::Cow;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use globset::{GlobBuilder, GlobSet, GlobSetBuilder};

use crate::error::{Result, SyncError};
use crate::lfs::pointer::{self, Pointer};
use crate::lfs::store::LfsStore;
use crate::profile::{LfsMode, SyncProfile};

/// Attribute line keeper maintains for an LFS-tracked pattern.
///
/// `diff=lfs merge=lfs` match what `git lfs track` writes, so a user running
/// the real client against the same repository sees a file it agrees with.
/// `-text` stops any EOL conversion touching binary content.
const ATTRIBUTE_SUFFIX: &str = "filter=lfs diff=lfs merge=lfs -text";

/// Marker keeper writes above the block it owns in `.gitattributes`.
const MANAGED_HEADER: &str = "# keeper-sync: managed LFS rules — edit above this line";

/// What staging decided for one path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StagedObject {
    /// Repository-relative path.
    pub path: PathBuf,
    pub oid: String,
    pub size: u64,
}

/// Everything the commit path needs to route large files through LFS.
#[derive(Debug, Default)]
pub struct LfsStaging {
    /// `path -> pointer bytes`, handed to `stage_and_commit` so the blob it
    /// writes is the pointer rather than the content.
    pub substitutions: BTreeMap<PathBuf, Vec<u8>>,
    /// Objects now in the local store that the remote has not seen.
    pub uploads: Vec<StagedObject>,
    /// `.gitattributes` changed and must be part of this commit.
    pub attributes_changed: bool,
}

/// Should this path be tracked through LFS?
///
/// Size alone, deliberately. Extension allow-lists are a support burden and get
/// the interesting case wrong: the 6 GB `.csv` export is exactly what must not
/// become a git blob.
pub fn applies(profile: &SyncProfile, size: u64) -> bool {
    profile.lfs_mode != LfsMode::Disabled && size >= profile.lfs_threshold_bytes
}

/// The size rule plus the profile's opt-out globs, compiled once per run.
///
/// Two things make the opt-out necessary in a repository that holds both notes
/// and media. First, the recorded `.gitattributes` rule is per-extension
/// ([`pattern_for`]), so a single oversized file converts its whole extension
/// for the rest of the repository's life. Second, the threshold that is right
/// for media is far below the size of a large text file — the lower it goes to
/// catch bulk, the likelier it is to swallow a format the user needs to diff.
///
/// Compiled once and reused: `prepare` walks every candidate, and building a
/// [`GlobSet`] per path would make the common case (no opt-out configured) pay
/// for a feature it does not use.
#[derive(Debug)]
pub struct LfsPolicy {
    threshold: u64,
    enabled: bool,
    never: Option<GlobSet>,
}

impl LfsPolicy {
    /// Glob semantics follow `.gitignore` and `.gitattributes`, because that is
    /// what anyone writing these patterns has already learned: a pattern with no
    /// `/` matches its basename at **any** depth (`*.md` covers
    /// `10-notes/a/b.md`), and a pattern containing `/` is anchored at the
    /// repository root (`10-notes/**` covers only that zone). Inventing a third
    /// dialect here would be a trap, not a feature.
    ///
    /// Refuses malformed globs rather than silently ignoring them — a typo in
    /// an opt-out that quietly does nothing is how a note ends up an opaque
    /// pointer months later.
    pub fn from_profile(profile: &SyncProfile) -> Result<Self> {
        let never = if profile.lfs_never.is_empty() {
            None
        } else {
            let mut builder = GlobSetBuilder::new();
            for pattern in &profile.lfs_never {
                let anchored: Cow<'_, str> = if pattern.contains('/') {
                    Cow::Borrowed(pattern.as_str())
                } else {
                    Cow::Owned(format!("**/{pattern}"))
                };
                let glob = GlobBuilder::new(&anchored)
                    .literal_separator(true)
                    .build()
                    .map_err(|err| {
                        SyncError::Config(format!("invalid lfsNever glob `{pattern}`: {err}"))
                    })?;
                builder.add(glob);
            }
            Some(builder.build().map_err(|err| {
                SyncError::Config(format!("could not compile lfsNever globs: {err}"))
            })?)
        };
        Ok(Self {
            threshold: profile.lfs_threshold_bytes,
            enabled: profile.lfs_mode != LfsMode::Disabled,
            never,
        })
    }

    /// `path` is repository-relative, which is what the globs are written
    /// against — matching an absolute path would make `*.md` depend on where
    /// the folder happens to be mounted.
    pub fn applies(&self, path: &Path, size: u64) -> bool {
        if !self.enabled || size < self.threshold {
            return false;
        }
        !self
            .never
            .as_ref()
            .is_some_and(|never| never.is_match(path))
    }
}

/// The `.gitattributes` pattern that covers `path`.
///
/// A per-extension rule (`*.mp4`) rather than a per-file one, so the file's
/// siblings are covered too and the block stays small on a media folder. A path
/// with no extension gets an exact-path rule.
pub fn pattern_for(path: &Path) -> String {
    match path.extension().and_then(|e| e.to_str()) {
        Some(ext) if !ext.is_empty() => pattern_for_extension(ext),
        // Escape nothing: a leading `/` anchors the pattern at the repository
        // root, which is what an exact-path rule means in gitattributes.
        _ => format!("/{}", path.to_string_lossy().replace('\\', "/")),
    }
}

/// The `.gitattributes` pattern that covers every file with this extension.
///
/// Split out of [`pattern_for`] for the caller that has no file yet: a
/// recording session writes its rule at session start (Story 41.5, FR-137), so
/// the working tree does not change under a running recorder, and at that
/// moment the only thing known about the segment is the extension it will
/// carry. One function so that the rule the session writes and the rule the
/// commit path would write are the same string by construction — two spellings
/// would each be idempotent against themselves and duplicate against the other.
///
/// `ext` is bare (`mp4`), and a leading dot is tolerated because half the
/// world's APIs return one.
pub fn pattern_for_extension(ext: &str) -> String {
    format!("*.{}", ext.trim().trim_start_matches('.'))
}

/// Ensure `.gitattributes` contains a rule for every pattern.
///
/// Returns whether the file changed. Idempotent, and it never reorders or
/// removes anything a human wrote: keeper's rules live in an appended block
/// below a marker, and existing coverage — however the user spelled it — is
/// respected rather than duplicated.
pub fn ensure_attributes(root: &Path, patterns: &[String]) -> Result<bool> {
    let file = root.join(".gitattributes");
    let existing = match std::fs::read_to_string(&file) {
        Ok(text) => text,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(err) => return Err(SyncError::io("read .gitattributes", file, err)),
    };

    let mut wanted: Vec<&String> = Vec::new();
    for pattern in patterns {
        let already = existing.lines().any(|line| {
            let line = line.trim();
            !line.starts_with('#')
                && line
                    .split_whitespace()
                    .next()
                    .is_some_and(|first| first == pattern)
                && line.contains("filter=lfs")
        });
        if !already && !wanted.contains(&pattern) {
            wanted.push(pattern);
        }
    }
    if wanted.is_empty() {
        return Ok(false);
    }

    let mut out = existing;
    if !out.is_empty() && !out.ends_with('\n') {
        out.push('\n');
    }
    if !out.contains(MANAGED_HEADER) {
        if !out.is_empty() {
            out.push('\n');
        }
        out.push_str(MANAGED_HEADER);
        out.push('\n');
    }
    for pattern in wanted {
        out.push_str(pattern);
        out.push(' ');
        out.push_str(ATTRIBUTE_SUFFIX);
        out.push('\n');
    }
    std::fs::write(&file, out).map_err(|err| SyncError::io("write .gitattributes", file, err))?;
    Ok(true)
}

/// Move a file's content into the LFS store and return its pointer.
///
/// Streams: the file is read in bounded chunks, hashed as it goes, and written
/// straight into the store's staging area. Nothing is ever fully buffered,
/// which is the whole point.
pub fn clean(store: &LfsStore, absolute: &Path) -> Result<Pointer> {
    let file = std::fs::File::open(absolute)
        .map_err(|err| SyncError::io("open for LFS staging", absolute, err))?;
    let size = file
        .metadata()
        .map_err(|err| SyncError::io("stat for LFS staging", absolute, err))?
        .len();
    let (oid, written) = store.insert_streaming(file)?;
    if written != size {
        // The file changed under us mid-read. Retryable, and the caller
        // requeues rather than committing a pointer to content that moved.
        return Err(SyncError::Integrity {
            subject: absolute.to_string_lossy().into_owned(),
            expected: format!("{size} bytes"),
            actual: format!("{written} bytes"),
        });
    }
    Ok(Pointer::new(oid, size))
}

/// The index key for a repository-relative path.
///
/// git spells every path with forward slashes, so a Windows `a\b` finds
/// nothing until it is asked for as `a/b`.
fn index_key(rela: &Path) -> String {
    rela.to_string_lossy().replace('\\', "/")
}

/// The pointer `blob` holds, if it is one.
///
/// The **blob's own** length decides it, read from the object header so
/// nothing larger is ever loaded. Consulting the index entry's `stat.size`
/// instead is the mistake this was born with: for an LFS entry that stat is
/// the WORKTREE file's, deliberately — see `git::commit::stage_and_commit` —
/// so it measures the gigabytes the pointer stands in for and never the ~130
/// bytes actually staged. It therefore rejected precisely the entries this
/// exists to recognise, and every real LFS file answered `None`.
fn pointer_blob(repo: &gix::Repository, blob: gix::hash::ObjectId) -> Option<Pointer> {
    if repo.find_header(blob).ok()?.size() > pointer::MAX_POINTER_BYTES as u64 {
        return None;
    }
    Pointer::parse(&repo.find_object(blob).ok()?.data)
}

/// Read the pointer recorded in the index for `rela`, if the blob is one.
///
/// Returns `None` for an ordinary file — the common case — after reading at
/// most [`pointer::MAX_POINTER_BYTES`] of object data, so this is safe to call
/// on any modified path.
pub fn indexed_pointer(repo: &gix::Repository, rela: &Path) -> Option<Pointer> {
    let index = repo.index_or_empty().ok()?;
    let key = index_key(rela);
    let entry = index.entry_by_path(gix::bstr::BStr::new(key.as_bytes()))?;
    pointer_blob(repo, entry.id)
}

/// Is `rela` reported as modified only because of git's racily-clean rule?
///
/// An entry whose mtime is not older than the index is re-read regardless of
/// its stat, and re-reading an LFS entry finds the worktree's gigabytes where
/// the blob holds a pointer. That difference is the design, not an edit, and
/// acting on it re-hashes the whole file to arrive back at the same pointer.
///
/// Three things must hold before a report is dismissed, and none of them reads
/// the file's content:
///
/// * The worktree's stat still matches the entry's. `gix::status` calls a path
///   unchanged exactly when that comparison passes and the entry is not racy,
///   so a match here means raciness was its only reason for speaking up. A
///   stat that differs is a genuine touch and must be re-staged even at
///   identical length — which is what a comparison against the pointer's
///   `size` alone could not tell apart, and it would have dropped an in-place
///   edit that happened to preserve the byte count for good. The repository's
///   own `core.trustCtime` / `core.checkStat` settings are read rather than
///   assumed, so the comparison is the same one status made.
/// * The staged blob is a pointer. For an ordinary blob, a racily-clean
///   re-read finding different bytes is a real edit caught inside the race
///   window, and swallowing it would lose the user's work.
/// * `HEAD` already records that same blob. `stage_and_commit` writes the
///   index before the commit so a crash between the two is re-driven on the
///   next pass (NFR-24); dismissing a path whose pointer is staged but not yet
///   committed would strand it until the file changed again.
pub fn is_false_modification(repo: &gix::Repository, rela: &Path, absolute: &Path) -> bool {
    let Ok(index) = repo.index_or_empty() else {
        return false;
    };
    let key = index_key(rela);
    let Some(entry) = index.entry_by_path(gix::bstr::BStr::new(key.as_bytes())) else {
        return false;
    };
    let Ok(options) = repo.stat_options() else {
        return false;
    };
    // The stat comparison leads because it costs one `lstat` and it is what
    // rejects an ordinary modified file, whose stat has moved. Both checks
    // below reach into the object database, and this keeps them off the path
    // every non-LFS modification takes.
    let unmoved = gix::index::fs::Metadata::from_path_no_follow(absolute)
        .ok()
        .and_then(|metadata| gix::index::entry::Stat::from_fs(&metadata).ok())
        .is_some_and(|worktree| worktree.matches(&entry.stat, options));
    if !unmoved || pointer_blob(repo, entry.id).is_none() {
        return false;
    }
    head_records(repo, rela, entry.id)
}

/// Does `HEAD` already record `blob` for `rela`?
///
/// An unborn branch, an unreadable tree and a path the commit does not carry
/// all answer no, which is the safe direction: the path is staged rather than
/// dismissed.
fn head_records(repo: &gix::Repository, rela: &Path, blob: gix::hash::ObjectId) -> bool {
    repo.head_tree().is_ok_and(|tree| {
        tree.lookup_entry_by_path(rela)
            .ok()
            .flatten()
            .is_some_and(|entry| entry.object_id() == blob)
    })
}

/// Prepare LFS staging for a set of candidate paths.
///
/// `candidates` are repository-relative paths already cleared by the
/// completeness gate. Paths under the threshold are left alone entirely.
pub fn prepare(
    profile: &SyncProfile,
    store: &LfsStore,
    candidates: &[PathBuf],
) -> Result<LfsStaging> {
    let mut staging = LfsStaging::default();
    if profile.lfs_mode == LfsMode::Disabled {
        return Ok(staging);
    }
    // Built before the walk so a malformed opt-out glob fails the run outright
    // rather than after an arbitrary number of files have already been routed.
    let policy = LfsPolicy::from_profile(profile)?;
    store.ensure_layout()?;

    let mut patterns: Vec<String> = Vec::new();
    for rela in candidates {
        let absolute = profile.local_path.join(rela);
        let metadata = match std::fs::symlink_metadata(&absolute) {
            Ok(metadata) => metadata,
            // Vanished since the gate cleared it: an ordinary outcome.
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => continue,
            Err(err) => return Err(SyncError::io("stat LFS candidate", absolute, err)),
        };
        // A symlink's blob is its target; its size is meaningless here.
        if !metadata.is_file() || !policy.applies(rela, metadata.len()) {
            continue;
        }

        let pointer = clean(store, &absolute)?;
        staging
            .substitutions
            .insert(rela.clone(), pointer.render().into_bytes());
        staging.uploads.push(StagedObject {
            path: rela.clone(),
            oid: pointer.oid.clone(),
            size: pointer.size,
        });
        let pattern = pattern_for(rela);
        if !patterns.contains(&pattern) {
            patterns.push(pattern);
        }
    }

    if !patterns.is_empty() {
        staging.attributes_changed = ensure_attributes(&profile.local_path, &patterns)?;
    }
    Ok(staging)
}

/// A worktree path still holding a pointer instead of its content.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingSmudge {
    pub path: PathBuf,
    pub pointer: Pointer,
}

/// Find every checked-out path whose worktree content is still a pointer.
///
/// After a fetch-and-apply the worktree receives whatever the blob holds — for
/// an LFS path that is the ~130-byte pointer, not the file. Materializing it is
/// the smudge direction, and it is the reason a peer can clone a profile and
/// get real bytes rather than text stubs.
///
/// Cheap to call: a candidate must be under 1 KiB before it is even read, so a
/// tree of ordinary files costs one `stat` each.
pub fn pending_smudges(root: &Path, tracked: &[PathBuf]) -> Result<Vec<PendingSmudge>> {
    let mut out = Vec::new();
    for rela in tracked {
        let absolute = root.join(rela);
        let metadata = match std::fs::symlink_metadata(&absolute) {
            Ok(metadata) => metadata,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => continue,
            Err(err) => return Err(SyncError::io("stat smudge candidate", absolute, err)),
        };
        if !metadata.is_file() || metadata.len() as usize > pointer::MAX_POINTER_BYTES {
            continue;
        }
        let bytes = match std::fs::read(&absolute) {
            Ok(bytes) => bytes,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => continue,
            Err(err) => return Err(SyncError::io("read smudge candidate", absolute, err)),
        };
        if !pointer::is_pointer_candidate(&bytes) {
            continue;
        }
        if let Some(parsed) = Pointer::parse(&bytes) {
            out.push(PendingSmudge {
                path: rela.clone(),
                pointer: parsed,
            });
        }
    }
    Ok(out)
}

/// Replace a pointer file with the object it names.
///
/// Writes to a sibling temp file and renames, so an interrupted materialization
/// leaves the pointer intact rather than a truncated video: the operation is
/// then simply retried. The staging name carries keeper's own `.keeper.*.tmp`
/// prefix, which tier 0 already excludes, so the watcher cannot mistake it for
/// user content.
pub fn materialize(store: &LfsStore, root: &Path, smudge: &PendingSmudge) -> Result<()> {
    let oid = &smudge.pointer.oid;
    if !store.contains(oid, smudge.pointer.size) {
        return Err(SyncError::Integrity {
            subject: format!("lfs object {oid}"),
            expected: format!("{} bytes in the local store", smudge.pointer.size),
            actual: "absent".to_owned(),
        });
    }
    let target = root.join(&smudge.path);
    let parent = target.parent().unwrap_or(root);
    let name = target
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "object".to_owned());
    let staging = parent.join(format!(".keeper.{name}.tmp"));

    std::fs::copy(store.object_path(oid), &staging)
        .map_err(|err| SyncError::io("stage lfs object", staging.clone(), err))?;
    std::fs::rename(&staging, &target)
        .map_err(|err| SyncError::io("publish lfs object", target.clone(), err))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn profile(root: &Path) -> SyncProfile {
        let mut p = SyncProfile::new("01J", "p", root, "https://git.invalid/r.git");
        p.lfs_threshold_bytes = 1024;
        p
    }

    #[test]
    fn only_files_at_or_above_the_threshold_are_tracked() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut p = profile(dir.path());
        assert!(!applies(&p, 1023));
        assert!(applies(&p, 1024), "the threshold is inclusive");
        assert!(applies(&p, 10_000));
        // Disabling LFS must disable it completely, not merely raise the bar.
        p.lfs_mode = LfsMode::Disabled;
        assert!(!applies(&p, u64::MAX));
    }

    #[test]
    fn an_opt_out_glob_beats_the_size_rule() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut p = profile(dir.path());
        p.lfs_never = vec!["*.md".into()];
        let policy = LfsPolicy::from_profile(&p).expect("policy");

        // This is the case the field exists for: the recorded .gitattributes
        // rule is per-extension, so without the opt-out one oversized note
        // converts *.md for the whole repository and every note stops being
        // diffable.
        assert!(!policy.applies(Path::new("10-notes/huge.md"), 10_000));
        assert!(policy.applies(Path::new("40-photos/shot.jpg"), 10_000));
        // The opt-out is not a way to disable the threshold.
        assert!(!policy.applies(Path::new("10-notes/small.md"), 10));
    }

    #[test]
    fn opt_out_globs_match_the_repository_relative_path() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut p = profile(dir.path());
        p.lfs_never = vec!["10-notes/**".into()];
        let policy = LfsPolicy::from_profile(&p).expect("policy");

        // Anchored at the repository root, so the rule does not depend on where
        // the folder happens to be mounted, and a same-named folder elsewhere
        // is unaffected.
        assert!(!policy.applies(Path::new("10-notes/a/b.md"), 10_000));
        assert!(policy.applies(Path::new("30-work/10-notes/a.md"), 10_000));
    }

    #[test]
    fn a_malformed_opt_out_glob_is_refused_rather_than_ignored() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut p = profile(dir.path());
        p.lfs_never = vec!["[unclosed".into()];
        // Silently dropping it would leave the user believing a format is
        // protected while every oversized file quietly converts its extension.
        let err = LfsPolicy::from_profile(&p).expect_err("must refuse");
        assert!(format!("{err}").contains("lfsNever"), "got: {err}");
    }

    #[test]
    fn no_opt_out_configured_behaves_exactly_like_the_size_rule() {
        let dir = tempfile::tempdir().expect("tempdir");
        let p = profile(dir.path());
        let policy = LfsPolicy::from_profile(&p).expect("policy");
        for size in [0_u64, 1023, 1024, 10_000] {
            assert_eq!(
                policy.applies(Path::new("any/file.bin"), size),
                applies(&p, size),
                "size {size} diverged from the plain threshold rule",
            );
        }
    }

    #[test]
    fn patterns_prefer_the_extension_so_siblings_are_covered() {
        assert_eq!(pattern_for(Path::new("media/clip.mp4")), "*.mp4");
        assert_eq!(pattern_for(Path::new("a/b/archive.tar.gz")), "*.gz");
        // No extension: anchor an exact path at the repository root.
        assert_eq!(pattern_for(Path::new("bin/blob")), "/bin/blob");
    }

    #[test]
    fn attributes_are_written_once_and_are_idempotent() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        assert!(ensure_attributes(root, &["*.mp4".into()]).expect("first"));
        let after_first = std::fs::read_to_string(root.join(".gitattributes")).expect("read");
        assert!(after_first.contains("*.mp4 filter=lfs diff=lfs merge=lfs -text"));

        assert!(
            !ensure_attributes(root, &["*.mp4".into()]).expect("second"),
            "re-running must not rewrite the file"
        );
        assert_eq!(
            after_first,
            std::fs::read_to_string(root.join(".gitattributes")).expect("read")
        );
    }

    #[test]
    fn a_users_own_rules_are_preserved_and_never_duplicated() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        std::fs::write(
            root.join(".gitattributes"),
            "# mine\n*.psd filter=lfs diff=lfs merge=lfs -text\n*.txt text\n",
        )
        .expect("seed");

        assert!(
            !ensure_attributes(root, &["*.psd".into()]).expect("existing coverage"),
            "a rule the user already wrote must be respected, not duplicated"
        );
        assert!(ensure_attributes(root, &["*.mp4".into()]).expect("new rule"));

        let text = std::fs::read_to_string(root.join(".gitattributes")).expect("read");
        assert_eq!(text.matches("*.psd").count(), 1);
        assert!(text.contains("# mine"), "the user's comment survives");
        assert!(text.contains("*.txt text"), "unrelated rules survive");
        assert!(text.contains(MANAGED_HEADER));
    }

    #[test]
    fn a_non_lfs_rule_for_the_same_pattern_does_not_count_as_coverage() {
        // `*.bin binary` is not LFS tracking, so the rule still has to be added.
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        std::fs::write(root.join(".gitattributes"), "*.bin binary\n").expect("seed");
        assert!(ensure_attributes(root, &["*.bin".into()]).expect("add"));
        let text = std::fs::read_to_string(root.join(".gitattributes")).expect("read");
        assert!(text.contains("*.bin filter=lfs"));
    }

    #[test]
    fn cleaning_streams_the_file_into_the_store_and_returns_its_pointer() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        let store = LfsStore::new(root.join("lfs"));
        store.ensure_layout().expect("layout");

        let payload = vec![9u8; 4096];
        let file = root.join("big.bin");
        std::fs::write(&file, &payload).expect("write");

        let pointer = clean(&store, &file).expect("clean");
        assert_eq!(pointer.size, 4096);
        assert!(store.contains(&pointer.oid, pointer.size));

        // The digest must be the real SHA-256 of the content, or the server
        // will reject the object.
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(&payload);
        assert_eq!(pointer.oid, hex::encode(hasher.finalize()));
        // The worktree file is untouched — the user still sees their data.
        assert_eq!(std::fs::read(&file).expect("read"), payload);
    }

    #[test]
    fn prepare_routes_only_oversized_files_and_leaves_the_rest_alone() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        let p = profile(root);
        let store = LfsStore::new(root.join(".git/lfs"));
        std::fs::write(root.join("small.txt"), vec![1u8; 100]).expect("write");
        std::fs::write(root.join("big.mp4"), vec![2u8; 5000]).expect("write");

        let staging = prepare(
            &p,
            &store,
            &[PathBuf::from("small.txt"), PathBuf::from("big.mp4")],
        )
        .expect("prepare");

        assert_eq!(staging.substitutions.len(), 1);
        assert!(staging.substitutions.contains_key(Path::new("big.mp4")));
        assert_eq!(staging.uploads.len(), 1);
        assert_eq!(staging.uploads[0].size, 5000);
        assert!(staging.attributes_changed);

        // The substituted blob really is a pointer, and a small one.
        let bytes = &staging.substitutions[Path::new("big.mp4")];
        assert!(bytes.len() < pointer::MAX_POINTER_BYTES);
        assert!(Pointer::parse(bytes).is_some());
    }

    #[test]
    fn prepare_does_nothing_at_all_when_lfs_is_disabled() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        let mut p = profile(root);
        p.lfs_mode = LfsMode::Disabled;
        std::fs::write(root.join("big.mp4"), vec![2u8; 5000]).expect("write");

        let staging = prepare(
            &p,
            &LfsStore::new(root.join(".git/lfs")),
            &[PathBuf::from("big.mp4")],
        )
        .expect("prepare");
        assert!(staging.substitutions.is_empty());
        assert!(staging.uploads.is_empty());
        assert!(!root.join(".gitattributes").exists());
    }
}
