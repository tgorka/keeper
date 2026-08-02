//! Releasing local LFS objects the remote already holds.
//!
//! # The duplication this exists to remove
//!
//! On the machine where content originates, an LFS-tracked file exists twice:
//! once in the worktree, and once in `<git-dir>/lfs/objects` as the byte-identical
//! object [`crate::lfs::stage::clean`] streamed there to compute its pointer.
//! That second copy is unavoidable at stage time — the bytes have to be read and
//! hashed to produce the pointer — but it is not needed forever. Measured on a
//! 211 GB archive: 215 GB of worktree content and 215 GB of store objects on one
//! 920 GB drive.
//!
//! # Why deleting is safe here, when `git lfs prune` has to be careful
//!
//! git-lfs prunes on a heuristic — "old enough, and the local ref is not ahead of
//! the remote" — and its known failure is deleting objects for staged files,
//! producing commits that can no longer be pushed (git-lfs#5636). It cannot do
//! better, because it has no record of which uploads actually landed.
//!
//! keeper does. Three conditions, each closing one of those holes:
//!
//! 1. **Nothing in the journal references the oid.** An owed upload is an exact
//!    answer, not an inference from ref positions. This also matters because the
//!    engine *rebuilds* an object it still owes — verified empirically: delete an
//!    object mid-upload and it is back within a minute, re-cleaned from the
//!    worktree. Pruning under an outstanding upload is therefore not dangerous,
//!    merely futile, and the condition keeps the two from fighting.
//! 2. **The worktree still holds the real content.** This is the condition
//!    git-lfs cannot express, and it is what makes the whole operation cheap to
//!    undo: the object is not the only local copy, the *file* is, and rebuilding
//!    it costs one local read. A path whose worktree content is pointer text is
//!    the inverse case — there the store object IS the only local copy, and it is
//!    never a candidate.
//! 3. **The remote confirms it holds the object.** Left to the caller, because it
//!    is the only condition that needs the network. It must be answered by the
//!    same upload-batch the upload path uses (an object with neither `actions`
//!    nor `error` means the server already has the content — see
//!    [`crate::lfs::batch`]), not by a cheaper existence probe that could be
//!    satisfied by something other than readable bytes.
//!
//! The planner answers 1 and 2 offline and hands the survivors up. Nothing here
//! deletes; [`release`] does, one object at a time, so a failure midway leaves a
//! consistent store rather than a half-applied plan.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use crate::error::{Result, SyncError};
use crate::lfs::pointer;
use crate::lfs::stage::indexed_pointer;
use crate::lfs::store::LfsStore;

/// A local object the planner believes may be released.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Releasable {
    pub oid: String,
    pub size: u64,
    /// The worktree path whose content can rebuild this object with no network.
    ///
    /// Carried rather than discarded so a caller can say *what* it is releasing,
    /// and so a future re-clean has a provenance trail in the log.
    pub rebuildable_from: PathBuf,
}

/// Which local objects are safe to release, ignoring the remote.
///
/// `owed` is every oid the journal still references — outstanding uploads and
/// queued downloads alike. `tracked` is the index's path list, which is where
/// the oid→path mapping comes from: the worktree file is real content, so the
/// only cheap way to learn its oid is the pointer recorded in the index.
///
/// Objects are deduplicated by oid. Two paths with identical content share one
/// object, and releasing it twice would be a spurious second delete.
pub fn plan(
    repo: &gix::Repository,
    root: &Path,
    store: &LfsStore,
    tracked: &[PathBuf],
    owed: &BTreeSet<String>,
) -> Result<Vec<Releasable>> {
    let mut found: BTreeMap<String, Releasable> = BTreeMap::new();

    for rela in tracked {
        let Some(recorded) = indexed_pointer(repo, rela) else {
            continue; // an ordinary file, not an LFS path
        };
        if owed.contains(&recorded.oid) || found.contains_key(&recorded.oid) {
            continue;
        }
        if !store.contains(&recorded.oid, recorded.size) {
            continue; // nothing to release
        }
        if !worktree_holds_content(root, rela, &recorded)? {
            continue;
        }
        found.insert(
            recorded.oid.clone(),
            Releasable {
                oid: recorded.oid,
                size: recorded.size,
                rebuildable_from: rela.clone(),
            },
        );
    }

    Ok(found.into_values().collect())
}

/// Does the worktree hold this pointer's real content, rather than pointer text?
///
/// Length leads because it settles every large file with one `lstat`: content
/// that is the recorded size cannot also be the ~130 bytes of a pointer. Only
/// when a genuinely small object could be confused with pointer text — possible
/// once the threshold is lowered towards it — is the file read at all, and then
/// at most [`pointer::MAX_POINTER_BYTES`] of it.
///
/// A mismatch is not an error: the file may be legitimately modified, mid-write,
/// or replaced by pointer text on a pointer-only profile. All of those mean "not
/// a candidate", which is the safe direction.
fn worktree_holds_content(root: &Path, rela: &Path, recorded: &pointer::Pointer) -> Result<bool> {
    let absolute = root.join(rela);
    let metadata = match std::fs::symlink_metadata(&absolute) {
        Ok(metadata) => metadata,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(err) => return Err(SyncError::io("stat prune candidate", absolute, err)),
    };
    if !metadata.is_file() || metadata.len() != recorded.size {
        return Ok(false);
    }
    if recorded.size > pointer::MAX_POINTER_BYTES as u64 {
        return Ok(true);
    }
    let head = match std::fs::read(&absolute) {
        Ok(bytes) => bytes,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(err) => return Err(SyncError::io("read prune candidate", absolute, err)),
    };
    Ok(!pointer::is_pointer_candidate(&head))
}

/// Delete `objects` from the store, returning how many bytes were reclaimed.
///
/// One object at a time and tolerant of a missing file: a concurrent re-clean or
/// a previous half-finished run should cost nothing. Any other IO error stops the
/// run — a store that cannot be written to is not a store that should keep being
/// pruned.
pub fn release(store: &LfsStore, objects: &[Releasable]) -> Result<u64> {
    let mut reclaimed = 0_u64;
    for object in objects {
        let path = store.object_path(&object.oid);
        match std::fs::remove_file(&path) {
            Ok(()) => reclaimed += object.size,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => continue,
            Err(err) => return Err(SyncError::io("release LFS object", path, err)),
        }
    }
    Ok(reclaimed)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `plan` needs a real index carrying a pointer blob, so its cases live in
    /// `tests/lfs_prune.rs` alongside the other repository-shaped fixtures.
    /// Releasing needs only a store, so it is covered here.
    #[test]
    fn release_reclaims_the_bytes_and_tolerates_a_missing_object() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = LfsStore::new(dir.path().join("lfs"));
        store.ensure_layout().expect("layout");
        let payload = vec![7_u8; 4096];
        let (oid, size) = store
            .insert_streaming(std::io::Cursor::new(payload))
            .expect("insert");

        let objects = vec![Releasable {
            oid: oid.clone(),
            size,
            rebuildable_from: PathBuf::from("big.bin"),
        }];
        assert_eq!(release(&store, &objects).expect("release"), size);
        assert!(!store.contains(&oid, size));
        // Idempotent: a concurrent re-clean, or a re-run after a partial
        // failure, must cost nothing rather than error.
        assert_eq!(release(&store, &objects).expect("second release"), 0);
    }
}
