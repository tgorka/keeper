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

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

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

/// The `.gitattributes` pattern that covers `path`.
///
/// A per-extension rule (`*.mp4`) rather than a per-file one, so the file's
/// siblings are covered too and the block stays small on a media folder. A path
/// with no extension gets an exact-path rule.
pub fn pattern_for(path: &Path) -> String {
    match path.extension().and_then(|e| e.to_str()) {
        Some(ext) if !ext.is_empty() => format!("*.{ext}"),
        // Escape nothing: a leading `/` anchors the pattern at the repository
        // root, which is what an exact-path rule means in gitattributes.
        _ => format!("/{}", path.to_string_lossy().replace('\\', "/")),
    }
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

/// Is `path` reported as modified only because of git's racily-clean rule?
///
/// Right after staging, the file's mtime is not older than the index, so git
/// re-reads content regardless of stat and sees the worktree bytes differ from
/// the pointer blob. That is not a modification.
///
/// The check is cheap: compare the worktree file's **size** against the size
/// recorded in the pointer. A real edit that preserves byte-length exactly and
/// leaves the content in the store is not distinguishable this way — but the
/// caller only reaches here for a path whose stat tuple the completeness gate
/// already saw as unchanged, so the length check is the last discriminator
/// needed rather than the only one.
pub fn is_false_modification(indexed: &Pointer, absolute: &Path) -> bool {
    std::fs::symlink_metadata(absolute).is_ok_and(|md| md.is_file() && md.len() == indexed.size)
}

/// Read the pointer recorded in the index for `path`, if the blob is one.
///
/// Returns `None` for an ordinary file — the common case — after reading at
/// most [`pointer::MAX_POINTER_BYTES`], so this is safe to call on every
/// modified path.
pub fn indexed_pointer(repo: &gix::Repository, rela: &Path) -> Option<Pointer> {
    let index = repo.index_or_empty().ok()?;
    let key = rela.to_string_lossy().replace('\\', "/");
    let entry = index.entry_by_path(gix::bstr::BStr::new(key.as_bytes()))?;
    // A pointer is under 1 KiB by specification, so an oversized blob cannot be
    // one and must not be loaded to find that out.
    if entry.stat.size as usize > pointer::MAX_POINTER_BYTES {
        return None;
    }
    let object = repo.find_object(entry.id).ok()?;
    Pointer::parse(&object.data)
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
        if !metadata.is_file() || !applies(profile, metadata.len()) {
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

    #[test]
    fn a_matching_length_marks_the_racily_clean_case_and_a_real_edit_does_not() {
        let dir = tempfile::tempdir().expect("tempdir");
        let file = dir.path().join("v.bin");
        std::fs::write(&file, vec![0u8; 500]).expect("write");
        let indexed = Pointer::new("a".repeat(64), 500);
        assert!(is_false_modification(&indexed, &file));

        std::fs::write(&file, vec![0u8; 501]).expect("grow");
        assert!(!is_false_modification(&indexed, &file));

        std::fs::remove_file(&file).expect("remove");
        assert!(!is_false_modification(&indexed, &file));
    }
}
