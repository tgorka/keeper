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
    /// LFS paths whose content is deliberately not on this machine.
    ///
    /// Counts **LFS paths only**, and that is the same restriction
    /// [`lfs::listing::collect`] documents: a path with no pointer in the index
    /// is not an LFS path, has no virtual/materialized state at all, and is
    /// therefore absent from both of these counts however large it is.
    ///
    /// "Tracked here, content deliberately not on this machine" — the worktree
    /// file *is* the committed pointer, which is what
    /// [`lfs::stage::worktree_pointer`] answers. This is the count that makes
    /// virtualization visible: `content` says what the folder weighs, `on_disk`
    /// says what it costs here, and this says how many paths explain the gap.
    pub virtual_paths: u64,
    /// LFS paths whose content is present: "tracked here, content present".
    ///
    /// A path whose worktree file is missing, or is there but is not a regular
    /// file, counts as **neither** this nor [`Footprint::virtual_paths`] — it is
    /// [`lfs::listing::LfsFileState::Absent`], and for that classifier's own
    /// reason: calling it materialized would put it in the count of paths that
    /// need no download. The two counts therefore need not add up to the number
    /// of LFS paths, and a reader who subtracts them gets the absent ones.
    pub materialized_paths: u64,
}

/// Measure `root`.
///
/// Almost metadata only: this walks and stats, and the only content it reads is
/// the first [`lfs::stage::MAX_POINTER_BYTES`] of a tracked LFS path whose
/// worktree file is small enough to *be* a pointer — the read that decides
/// virtual from materialized, and the same one `lfs::listing::collect` pays. A
/// materialized large file short-circuits on its length and is never opened, so
/// the cost is bounded by the number of pointer-sized files rather than by the
/// size of the folder. Said precisely because the earlier "reads no file's
/// content" was a perf contract a later caller would have relied on. On the
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
        virtual_paths: 0,
        materialized_paths: 0,
    };
    // Best effort, and last: a repository this cannot open still has a size, and
    // refusing to report it because one of four numbers is unavailable would
    // answer a question with silence.
    if let Ok(repo) = git::repo::open(root, false) {
        if let Ok(tracked) = git::repo::tracked_paths(&repo) {
            if let Ok(plan) = lfs::prune::plan(&repo, root, &store, &tracked, &Default::default()) {
                out.reclaimable = plan.iter().map(|entry| entry.size).sum();
            }
            let tally = tracked_tally(&repo, root, &tracked);
            out.content = tally.content;
            out.virtual_paths = tally.virtual_paths;
            out.materialized_paths = tally.materialized_paths;
        }
    }
    Ok(out)
}

/// What one pass over the tracked set found.
///
/// A struct rather than three walks: [`measure`] wants the sizes and the two
/// counts, and every one of them is decided by the same pointer lookup and the
/// same `stat` on the same path. Returning them together is what keeps the cost
/// one syscall per path.
#[derive(Debug, Default, PartialEq, Eq)]
struct Tally {
    content: u64,
    virtual_paths: u64,
    materialized_paths: u64,
}

/// The full size of everything tracked, whether or not it is materialised, and
/// how many of the LFS paths are holding their content here.
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
///
/// Only the LFS half is classified, and only that half *can* be: a path with no
/// pointer in the index is not an LFS path and has no virtual/materialized
/// state to report. The classification is [`lfs::listing::collect`]'s, arm for
/// arm and in the same order, because the Files pane renders that one and two
/// surfaces disagreeing about whether a path is virtual would be worse than
/// either. That is also why it is `metadata` and not `symlink_metadata` here:
/// the listing documents that choice as a decision — it is the call
/// `browse::list_resolved` binds — and a tracked symlink to pointer text must
/// not read `virtual` on one surface and `materialized` on the other.
fn tracked_tally(repo: &gix::Repository, root: &Path, tracked: &[std::path::PathBuf]) -> Tally {
    let mut out = Tally::default();
    for rela in tracked {
        let absolute = root.join(rela);
        let Some(pointer) = lfs::stage::indexed_pointer(repo, rela) else {
            // Not an LFS path: the worktree is the only place its size is
            // written down, and there is nothing to classify.
            out.content += std::fs::symlink_metadata(&absolute)
                .map(|meta| meta.len())
                .unwrap_or(0);
            continue;
        };
        out.content += pointer.size;
        // The one `stat` this path pays, and it settles the classification.
        match std::fs::metadata(&absolute) {
            // No file, or something that is not one: `Absent` on the listing,
            // and neither count here.
            Err(_) => {}
            Ok(meta) if !meta.is_file() => {}
            Ok(meta) if lfs::stage::worktree_pointer(&absolute, &meta).is_some() => {
                out.virtual_paths += 1;
            }
            Ok(_) => out.materialized_paths += 1,
        }
    }
    out
}

/// Tracked files that are plain blobs in git and would be LFS objects today.
///
/// The threshold is a per-machine setting and it moves. A file committed when
/// it was 4 MiB stays a blob for the life of the history, whatever the setting
/// says afterwards — git has no way to change its mind about a commit that
/// already happened. So the drive quietly carries a set of files the current
/// rule would have sent to LFS, and nothing anywhere says so.
///
/// This is the check somebody would otherwise run by hand and only after they
/// already suspected something. Returns `(count, bytes)`.
///
/// It reads the index rather than the worktree: a materialised LFS file IS the
/// real bytes on disk, so a size check out there answers a different question
/// and answers it wrongly.
pub fn blobs_over_threshold(
    repo: &gix::Repository,
    root: &Path,
    tracked: &[std::path::PathBuf],
    threshold: u64,
) -> (usize, u64) {
    let mut count = 0usize;
    let mut bytes = 0u64;
    for rela in tracked {
        if lfs::stage::indexed_pointer(repo, rela).is_some() {
            continue; // already an LFS object
        }
        let Ok(meta) = std::fs::symlink_metadata(root.join(rela)) else {
            continue;
        };
        if meta.is_file() && meta.len() >= threshold {
            count += 1;
            bytes += meta.len();
        }
    }
    (count, bytes)
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

    /// Write a repository whose index records each `(path, blob)` pair, paired
    /// with whatever that path's worktree file is at the moment the entry is
    /// pushed.
    ///
    /// Built by hand rather than through `git add`, exactly as `lfs::stage`'s
    /// own fixtures are: pointer text only reaches the index through a filter
    /// this test is not installing, and the state under test is precisely "the
    /// index says pointer, the worktree says something else". Every path must
    /// already exist on disk — `Stat::from_fs` needs it — so a test that wants
    /// a missing file deletes it after this returns, which is also the order
    /// reality produces it in. Nothing is returned because [`measure`] opens the
    /// repository itself, from the path.
    fn repo_with_index(root: &Path, entries: &[(&str, Vec<u8>)]) {
        use gix::index::{entry::Flags, entry::Mode, entry::Stat, State};

        std::process::Command::new("git")
            .args(["init", "-q", "-b", "main"])
            .current_dir(root)
            .status()
            .expect("git init");
        let repo = git::repo::open(root, false).expect("open");
        let mut index = State::new(repo.object_hash());
        for (rela, blob_bytes) in entries {
            let blob = repo.write_blob(blob_bytes).expect("write blob").detach();
            let metadata = gix::index::fs::Metadata::from_path_no_follow(&root.join(rela))
                .expect("stat the worktree");
            let stat = Stat::from_fs(&metadata).expect("stat convert");
            index.dangerously_push_entry(stat, blob, Flags::empty(), Mode::FILE, (*rela).into());
        }
        index.sort_entries();
        let mut file = gix::index::File::from_state(index, repo.index_path());
        file.write(gix::index::write::Options::default())
            .expect("write index");
    }

    /// The bytes a committed LFS pointer of `size` renders to: the index blob of
    /// every LFS path below, and the worktree content of a virtual one.
    fn pointer_bytes(oid_char: &str, size: u64) -> Vec<u8> {
        lfs::pointer::Pointer::new(oid_char.repeat(64), size)
            .render()
            .into_bytes()
    }

    /// The two counts are a partition of the LFS paths by *where the content
    /// is*, and they are the fact that makes virtualization visible at all: the
    /// folder weighs `content` and costs far less here, and these say how many
    /// paths explain it.
    ///
    /// Deliberately **two** materialized paths against one virtual. Equal counts
    /// would pass with the two arms swapped, and "which of these two numbers is
    /// which" is the only thing this classification can get wrong.
    ///
    /// `content` is unchanged by their arrival — it is still read off the
    /// pointers, so the virtual path counts for the 9 kB it weighs on the server
    /// and not the 130 bytes of pointer text standing in for it.
    #[test]
    fn the_two_counts_split_the_lfs_paths_by_where_the_content_is() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        let held_bytes = pointer_bytes("a", 5_000);
        let also_held_bytes = pointer_bytes("b", 3_000);
        let away_bytes = pointer_bytes("c", 9_000);
        // Materialized: the worktree holds the real content, which is far too
        // large to be pointer text.
        std::fs::write(root.join("held.bin"), vec![1u8; 5_000]).expect("write held");
        std::fs::write(root.join("also-held.bin"), vec![2u8; 3_000]).expect("write also-held");
        // Virtual: the worktree file IS the committed pointer.
        std::fs::write(root.join("away.bin"), &away_bytes).expect("write away");
        repo_with_index(
            root,
            &[
                ("also-held.bin", also_held_bytes),
                ("away.bin", away_bytes),
                ("held.bin", held_bytes),
            ],
        );

        let f = measure(root).expect("measure");

        assert_eq!(
            f.materialized_paths, 2,
            "the two paths holding their content"
        );
        assert_eq!(
            f.virtual_paths, 1,
            "the one path deliberately not holding it"
        );
        assert_eq!(
            f.content, 17_000,
            "and every one of them is still measured from its pointer, so the \
             folder's weight is what the server holds"
        );
    }

    /// `Absent` is neither, and that is the classifier's own decision: calling a
    /// path whose file is gone `materialized` would put it in the count of paths
    /// that need no download, which is the opposite of true.
    ///
    /// It still counts for its size. The index and the worktree disagree
    /// constantly on a folder somebody is using, and a footprint that dropped
    /// the weight of every mid-edit path would report a folder as lighter than
    /// the server's copy of it.
    #[test]
    fn a_path_whose_worktree_file_is_gone_counts_as_neither() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        let gone_bytes = pointer_bytes("c", 7_000);
        std::fs::write(root.join("gone.bin"), &gone_bytes).expect("write gone");
        repo_with_index(root, &[("gone.bin", gone_bytes)]);
        std::fs::remove_file(root.join("gone.bin")).expect("delete gone");

        let f = measure(root).expect("measure");

        assert_eq!(f.virtual_paths, 0);
        assert_eq!(f.materialized_paths, 0);
        assert_eq!(
            f.content, 7_000,
            "a missing path still weighs what its pointer says"
        );
    }

    /// A path with no pointer in the index is not an LFS path, has no
    /// virtual/materialized state, and belongs to neither count however large it
    /// is — the same restriction `lfs::listing::collect` documents, which is why
    /// the Files pane has no row for it either.
    #[test]
    fn a_tracked_file_that_is_not_an_lfs_path_is_in_neither_count() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        std::fs::write(root.join("note.md"), b"hello").expect("write note");
        repo_with_index(root, &[("note.md", b"hello".to_vec())]);

        let f = measure(root).expect("measure");

        assert_eq!(f.virtual_paths, 0);
        assert_eq!(f.materialized_paths, 0);
        assert_eq!(
            f.content, 5,
            "and it is measured from the worktree, where a tracked file's bytes \
             are its bytes"
        );
    }
}
