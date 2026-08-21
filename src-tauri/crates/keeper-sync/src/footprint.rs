//! What a synced folder costs on this disk, and how much of that is a copy.
//!
//! # The question this answers
//!
//! "neuradrive is 220 GB here and less on the server." Both true, and the gap
//! is not a bug — a synced folder holds the working tree *and* a local LFS cache
//! of the same content, so a folder of large files is close to twice its own
//! size by design. What was missing is any way to see that from inside keeper,
//! which left the difference looking like a leak.
//!
//! # Why there is no server total here
//!
//! There is no generic way to ask a git remote how large it is: the protocol has
//! no such request, and `ls-remote` answers with names, not bytes. A host's own
//! API can answer it — Forgejo's does — but keeper syncs to plain git and a
//! number that only appeared for one host would be worse than none.
//!
//! What *is* knowable without asking anyone is the half that explains the gap:
//! the bytes held locally that the remote already has, which is exactly what
//! `lfs::prune::plan` computes in order to release them. So this reports what
//! the folder costs and how much of that cost is redundant, which is the
//! actionable form of the question.

use std::path::Path;

use crate::error::Result;
use crate::git;
use crate::lfs;

/// A folder's cost on disk, decomposed.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Footprint {
    /// Everything under the folder, working tree and `.git` together.
    pub on_disk: u64,
    /// The local LFS object cache — the second copy of large content.
    pub lfs_cache: u64,
    /// Cache the remote already holds, which keeper may release at any time.
    pub reclaimable: u64,
    /// Transfer scratch left by interrupted runs.
    pub scratch: u64,
    /// Everything this folder tracks, at full size, whether or not its bytes
    /// are on this machine.
    ///
    /// This is the number the remote holds, computed without asking it. An LFS
    /// pointer carries the size of the object it stands for, so a folder that
    /// has never downloaded a single video still knows exactly what it would
    /// weigh — and `content - on_disk` is what a virtual folder would be
    /// leaving on the server.
    pub content: u64,
}

/// Measure `root`.
///
/// Metadata only: this walks and stats, and reads no file's content. On the
/// folder that prompted it — 210 GB, a few thousand entries — that is a
/// sub-second walk, which is why it can be asked for on demand rather than
/// cached and served stale.
pub fn measure(root: &Path) -> Result<Footprint> {
    let git_dir = root.join(".git");
    let store = lfs::store::LfsStore::in_git_dir(git_dir.clone());
    let mut out = Footprint {
        on_disk: tree_bytes(root),
        lfs_cache: tree_bytes(&git_dir.join("lfs").join("objects")),
        scratch: tree_bytes(&store.tmp_dir()) + tree_bytes(&git_dir.join("lfs").join("incomplete")),
        reclaimable: 0,
        content: 0,
    };
    // Best effort, and last: a repository this cannot open still has a size, and
    // refusing to report it because one of four numbers is unavailable would
    // answer a question with silence.
    if let Ok(repo) = git::repo::open(root, false) {
        if let Ok(tracked) = git::repo::tracked_paths(&repo) {
            if let Ok(plan) = lfs::prune::plan(&repo, root, &store, &tracked, &Default::default()) {
                out.reclaimable = plan.iter().map(|entry| entry.size).sum();
            }
            out.content = tracked_content(&repo, root, &tracked);
        }
    }
    Ok(out)
}

/// The full size of everything tracked, whether or not it is materialised.
///
/// Two kinds of path, and the difference is the whole point. An LFS path is
/// measured from its pointer, which states the object's size — so a file whose
/// bytes were never fetched, or were released to reclaim space, still counts
/// for what it actually weighs. Every other path is measured from the worktree,
/// where a tracked file's bytes are its bytes.
///
/// A path that has gone missing contributes nothing rather than failing the
/// measurement: the index and the worktree disagree constantly on a folder
/// somebody is using, and a footprint is not an audit.
fn tracked_content(repo: &gix::Repository, root: &Path, tracked: &[std::path::PathBuf]) -> u64 {
    tracked
        .iter()
        .map(|rela| match lfs::stage::indexed_pointer(repo, rela) {
            Some(pointer) => pointer.size,
            None => std::fs::symlink_metadata(root.join(rela))
                .map(|meta| meta.len())
                .unwrap_or(0),
        })
        .sum()
}

/// Bytes under a directory, following nothing and failing on nothing.
///
/// A folder being measured is a folder somebody is also using: files appear and
/// vanish under the walk, and a measurement that errored on a file that was
/// deleted between `read_dir` and `metadata` would fail exactly when the folder
/// is busiest. An approximate answer is the honest one here — this is a
/// footprint, not an audit.
fn tree_bytes(dir: &Path) -> u64 {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return 0;
    };
    let mut total = 0;
    for entry in entries.flatten() {
        let Ok(meta) = entry.metadata() else { continue };
        if meta.is_dir() {
            total += tree_bytes(&entry.path());
        } else if meta.is_file() {
            total += meta.len();
        }
        // Symlinks contribute nothing: a link's target is either inside this
        // tree and already counted, or outside it and not this folder's cost.
    }
    total
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_folder_reports_what_is_under_it() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(dir.path().join("a.bin"), vec![0u8; 1000]).expect("write");
        std::fs::create_dir_all(dir.path().join("deep/deeper")).expect("mkdir");
        std::fs::write(dir.path().join("deep/deeper/b.bin"), vec![0u8; 2000]).expect("write");

        assert_eq!(
            tree_bytes(dir.path()),
            3000,
            "nested content is part of the cost"
        );
    }

    #[test]
    fn a_directory_that_is_not_there_costs_nothing() {
        assert_eq!(tree_bytes(Path::new("/no/such/path/at/all")), 0);
    }

    /// The cache and the scratch are counted separately AND are inside
    /// `on_disk`: the card says "210 GB, 92 of it a second copy", not "210 plus
    /// 92". A reader adding the parts and getting more than the total would
    /// stop trusting the row.
    #[test]
    fn the_parts_are_inside_the_total_not_beside_it() {
        let dir = tempfile::tempdir().expect("tempdir");
        let objects = dir.path().join(".git/lfs/objects/ab");
        std::fs::create_dir_all(&objects).expect("mkdir");
        std::fs::write(objects.join("abc"), vec![0u8; 5000]).expect("write");
        std::fs::write(dir.path().join("note.md"), vec![0u8; 100]).expect("write");

        let f = measure(dir.path()).expect("measure");

        assert_eq!(f.on_disk, 5100);
        assert_eq!(f.lfs_cache, 5000);
        assert!(
            f.lfs_cache < f.on_disk,
            "the cache is part of the folder, not extra to it"
        );
    }
}
