//! "Reset this copy": a phone's way out of a divergence (AD-199).
//!
//! A phone merges nothing. When its branch and the remote's have both moved,
//! [`crate::engine::phone_divergence_sentence`] names the two ways out — push
//! the phone's commits from a Mac, or reset this copy — and until this module
//! the second was a sentence with no verb behind it. On kalypso (2026-10-18)
//! the owner's two drives sat diverged until somebody moved the branches by
//! hand over `devicectl`.
//!
//! # What a reset promises
//!
//! **The remote wins every path, and nothing the phone held is lost.**
//!
//! * The branch, the index and the worktree end exactly where the remote's
//!   tip is — the same writes a fast-forward makes
//!   ([`super::repo::move_branch_to`]), so a path the remote holds is written
//!   with the remote's bytes and nothing the remote has is deleted.
//! * The phone's history is kept under `refs/keeper/reset/<stamp>`
//!   ([`SAVED_PREFIX`]), so every commit it made stays reachable — and
//!   recoverable from a Mac that fetches the ref — however the files below
//!   fare. It is a local ref: nothing pushes it.
//! * Every file the phone's own commits changed, and that the remote holds
//!   differently, is copied **beside** the remote's version under a conflict
//!   name ([`super::conflict::conflict_name`]) before the worktree moves —
//!   AD-43's shape, the one a Mac's merge leaves, so a person meets the same
//!   pair whichever device resolved it. A file only the phone holds stays at
//!   its own path. The next pass commits both as ordinary new files and pushes
//!   them, which is how the phone's writes reach the remote at all.
//!
//! "The phone's own commits" is decided against the merge base: a path whose
//! blob at the phone's tip is the base's blob was not changed here, and a copy
//! of it would be the remote's own older revision — litter. Two histories with
//! no common commit (the parentless root a phone seeded before its first clone,
//! which is how kalypso diverged) have no base, and then every path of the
//! phone's tree is the phone's.
//!
//! A phone deletion is not re-applied: a deletion carries no content, and the
//! remote's file coming back costs a person a delete, where the other way round
//! would cost them a file (AD-43's "a modification always beats a deletion").
//!
//! # Where the copies come from
//!
//! The worktree, not the object database. The engine commits before it resets,
//! so the file at the path *is* the phone's committed revision as it was
//! checked out — and that is what a person would recognise. A blob read out of
//! the store would be the LFS pointer for a large file the phone had
//! materialized. A path that is not on disk (a symlink, a gitlink, a file gone
//! since the commit) is not copied; it is in the saved ref, and it is counted
//! ([`Reset::only_saved`]) so the caller can say so.

use std::{
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
};

use super::repo::{head_commit_id, move_branch_to, workdir, FastForward};
use crate::error::{Result, SyncError};

/// Where a reset keeps the history it moved the branch off. One ref per reset,
/// named by its stamp, so two resets never overwrite each other's rescue.
pub const SAVED_PREFIX: &str = "refs/keeper/reset/";

/// What [`reset_to`] did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Reset {
    /// The reference now holding the phone's former tip; `None` when there was
    /// nothing to keep — an unborn branch, or one already at the target.
    pub saved: Option<String>,
    /// Conflict copies written beside a path the remote also holds,
    /// repository-relative.
    pub copies: Vec<PathBuf>,
    /// Files only this copy held, left at their own path, repository-relative.
    pub kept: Vec<PathBuf>,
    /// The phone's revisions that were not on disk to copy; they are in
    /// [`Self::saved`] and nowhere else.
    pub only_saved: Vec<PathBuf>,
    /// What moving the worktree onto the target wrote and removed.
    pub moved: FastForward,
}

/// What becomes of one of the phone's revisions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Keep {
    /// The remote holds the path too: the phone's bytes go beside it.
    Beside,
    /// Only the phone holds it: it stays where it is.
    InPlace,
}

/// Move this repository's branch onto `target`, keeping the phone's history
/// under [`SAVED_PREFIX`] and its files beside the remote's. See the module
/// docs for the promise and the order.
///
/// `stamp` and `device` name the copies exactly as the Mac's merge names its
/// own (`yyyymmdd-hhmmss`, the device label). A local change git has not seen
/// on a path this would write refuses the whole reset with a transient error,
/// [`super::repo::fast_forward`]'s refusal, and every copy made for it is
/// removed again; the saved ref stays, because it is the phone's history and
/// costs nothing.
///
/// Blocking.
pub fn reset_to(
    repo: &gix::Repository,
    target: gix::ObjectId,
    stamp: &str,
    device: &str,
    interrupt: &AtomicBool,
) -> Result<Reset> {
    let Some(head) = head_commit_id(repo)? else {
        // Nothing of the phone's to keep: adopting the remote is all there is.
        let moved = move_branch_to(repo, target, interrupt, REFLOG)?;
        return Ok(Reset {
            moved,
            ..Reset::default()
        });
    };
    if head == target {
        return Ok(Reset::default());
    }
    let root = workdir(repo)?;
    let plan = phone_revisions(repo, head, target)?;
    let saved = save_history(repo, head, stamp)?;

    let mut written: Vec<(PathBuf, PathBuf, Keep)> = Vec::new();
    let mut only_saved = Vec::new();
    for (rela, keep) in plan {
        if interrupt.load(Ordering::Relaxed) {
            remove_copies(&root, &written);
            return Err(SyncError::Cancelled);
        }
        match copy_beside(&root, &rela, stamp, device) {
            Ok(Some(copy)) => written.push((rela, copy, keep)),
            Ok(None) => only_saved.push(rela),
            Err(err) => {
                remove_copies(&root, &written);
                return Err(err);
            }
        }
    }

    let moved = match move_branch_to(repo, target, interrupt, REFLOG) {
        Ok(moved) => moved,
        Err(err) => {
            remove_copies(&root, &written);
            return Err(err);
        }
    };

    // A file only the phone held was removed with the phone's tree; its copy
    // goes back to its own name — unless the remote has put something there
    // since (a directory where the phone had a file), in which case the copy
    // is where it stays.
    let mut copies = Vec::new();
    let mut kept = Vec::new();
    for (rela, copy, keep) in written {
        if keep == Keep::InPlace {
            let path = root.join(&rela);
            if std::fs::symlink_metadata(&path).is_err() {
                if let Some(parent) = path.parent() {
                    std::fs::create_dir_all(parent).map_err(|err| {
                        SyncError::io("recreate a kept file's folder", parent, err)
                    })?;
                }
                std::fs::rename(root.join(&copy), &path)
                    .map_err(|err| SyncError::io("put a kept file back", &path, err))?;
                kept.push(rela);
                continue;
            }
        }
        copies.push(copy);
    }
    Ok(Reset {
        saved: Some(saved),
        copies,
        kept,
        only_saved,
        moved,
    })
}

/// The reflog line a reset leaves on the branch.
const REFLOG: &str = "keeper: reset this copy to the remote";

/// The phone's revisions the remote does not hold, and what becomes of each.
///
/// Index-shaped comparisons rather than a tree diff: three flat sorted lists
/// of `(path, id, mode)`, which is what [`super::repo::fast_forward`] already
/// builds for the same trees, and a lookup by path is a binary search.
fn phone_revisions(
    repo: &gix::Repository,
    head: gix::ObjectId,
    target: gix::ObjectId,
) -> Result<Vec<(PathBuf, Keep)>> {
    let base = match repo.merge_base(head, target) {
        Ok(base) => Some(base.detach()),
        Err(gix::repository::merge_base::Error::NotFound { .. }) => None,
        Err(err) => {
            return Err(SyncError::Git(format!(
                "could not read the merge base: {}",
                super::fetch::flatten(&err)
            )))
        }
    };
    let ours = index_of(repo, head)?;
    let theirs = index_of(repo, target)?;
    let base = base.map(|base| index_of(repo, base)).transpose()?;
    let same = |a: &gix::index::Entry, b: &gix::index::Entry| a.id == b.id && a.mode == b.mode;

    let mut plan = Vec::new();
    for entry in ours.entries() {
        // A submodule's commit is not a file anybody could open.
        if entry.mode == gix::index::entry::Mode::COMMIT {
            continue;
        }
        let rela = entry.path(&ours);
        if base
            .as_ref()
            .and_then(|base| base.entry_by_path(rela))
            .is_some_and(|before| same(before, entry))
        {
            continue;
        }
        let keep = match theirs.entry_by_path(rela) {
            Some(remote) if same(remote, entry) => continue,
            Some(_) => Keep::Beside,
            None => Keep::InPlace,
        };
        plan.push((gix::path::from_bstr(rela).into_owned(), keep));
    }
    Ok(plan)
}

fn index_of(repo: &gix::Repository, commit: gix::ObjectId) -> Result<gix::index::File> {
    let tree = repo
        .find_commit(commit)
        .map_err(|err| {
            SyncError::Git(format!(
                "could not read commit {commit}: {}",
                super::fetch::flatten(&err)
            ))
        })?
        .tree_id()
        .map_err(|err| SyncError::Git(format!("could not read the tree of {commit}: {err}")))?
        .detach();
    repo.index_from_tree(&tree)
        .map_err(|err| SyncError::Git(format!("could not list the tree of {commit}: {err}")))
}

/// Point a new `refs/keeper/reset/<stamp>` at `head`, counting up from `-2`
/// should two resets share a second.
fn save_history(repo: &gix::Repository, head: gix::ObjectId, stamp: &str) -> Result<String> {
    use gix::refs::transaction::{Change, LogChange, PreviousValue, RefEdit, RefLog};
    for ordinal in 1..=99u32 {
        let name = if ordinal == 1 {
            format!("{SAVED_PREFIX}{stamp}")
        } else {
            format!("{SAVED_PREFIX}{stamp}-{ordinal}")
        };
        if repo
            .try_find_reference(name.as_str())
            .ok()
            .flatten()
            .is_some()
        {
            continue;
        }
        let full = gix::refs::FullName::try_from(name.as_str())
            .map_err(|err| SyncError::Git(format!("{name} is not a reference name: {err}")))?;
        repo.edit_reference(RefEdit {
            change: Change::Update {
                log: LogChange {
                    mode: RefLog::AndReference,
                    force_create_reflog: false,
                    message: "keeper: the phone's history before a reset".into(),
                },
                expected: PreviousValue::MustNotExist,
                new: gix::refs::Target::Object(head),
            },
            name: full,
            deref: false,
        })
        .map_err(|err| SyncError::Git(format!("could not keep the phone's history: {err}")))?;
        return Ok(name);
    }
    Err(SyncError::Git(format!(
        "could not keep the phone's history: every {SAVED_PREFIX}{stamp} name is taken"
    )))
}

/// Copy the file at `rela` to a conflict name beside it, never over anything:
/// the copy is opened `create_new` and the name counts up from 2 on a
/// collision, the engine's rule for its own copies. `None` when there is no
/// regular file at `rela` to copy.
fn copy_beside(root: &Path, rela: &Path, stamp: &str, device: &str) -> Result<Option<PathBuf>> {
    let source = root.join(rela);
    match std::fs::symlink_metadata(&source) {
        Ok(meta) if meta.is_file() => {}
        Ok(_) => return Ok(None),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(SyncError::io("read a file the reset keeps", source, err)),
    }
    let parent = rela.parent().unwrap_or_else(|| Path::new(""));
    for ordinal in 1..=99u32 {
        let name = if ordinal == 1 {
            super::conflict::conflict_name(rela, stamp, device)
        } else {
            super::conflict::conflict_name_numbered(rela, stamp, device, ordinal)
        };
        let copy = parent.join(name);
        let path = root.join(&copy);
        let mut file = match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
        {
            Ok(file) => file,
            Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(err) => return Err(SyncError::io("write a conflict copy", path, err)),
        };
        // Streamed: a file the phone materialized can be a video, and a
        // phone has no memory to hold one twice.
        let mut reader = std::fs::File::open(&source)
            .map_err(|err| SyncError::io("read a file the reset keeps", &source, err))?;
        if let Err(err) = std::io::copy(&mut reader, &mut file).and_then(|_| file.sync_all()) {
            let _ = std::fs::remove_file(&path);
            return Err(SyncError::io("write a conflict copy", path, err));
        }
        return Ok(Some(copy));
    }
    Err(SyncError::Git(format!(
        "could not name a copy of {}: every conflict name is taken",
        rela.display()
    )))
}

/// Undo [`copy_beside`] for a reset that did not happen: after it the phone's
/// files are at their own paths again, as they were.
fn remove_copies(root: &Path, written: &[(PathBuf, PathBuf, Keep)]) {
    for (_, copy, _) in written {
        let path = root.join(copy);
        if let Err(err) = std::fs::remove_file(&path) {
            tracing::warn!(path = %path.display(), error = %err, "could not remove a reset's copy");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A repository driven by plain `git`: `None` where there is none on
    /// `PATH`, the convention every fixture in this crate follows.
    struct Fixture {
        _dir: tempfile::TempDir,
        root: PathBuf,
    }

    impl Fixture {
        fn new() -> Option<Self> {
            let dir = tempfile::tempdir().expect("tempdir");
            let root = dir.path().join("copy");
            std::fs::create_dir_all(&root).expect("root");
            let fixture = Self { _dir: dir, root };
            if !fixture.try_git(&["init", "-q", "-b", "main"]) {
                return None;
            }
            fixture.git(&["config", "user.email", "t@keeper.invalid"]);
            fixture.git(&["config", "user.name", "t"]);
            Some(fixture)
        }

        fn try_git(&self, args: &[&str]) -> bool {
            std::process::Command::new("git")
                .args(args)
                .current_dir(&self.root)
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status()
                .is_ok_and(|status| status.success())
        }

        fn git(&self, args: &[&str]) {
            assert!(self.try_git(args), "git {args:?}");
        }

        fn write(&self, rela: &str, bytes: &[u8]) {
            let path = self.root.join(rela);
            std::fs::create_dir_all(path.parent().expect("parent")).expect("dirs");
            std::fs::write(path, bytes).expect("write");
        }

        fn read(&self, rela: &str) -> Vec<u8> {
            std::fs::read(self.root.join(rela)).unwrap_or_else(|err| panic!("{rela}: {err}"))
        }

        fn commit(&self, subject: &str) -> gix::ObjectId {
            self.git(&["add", "-A"]);
            self.git(&["commit", "-qm", subject]);
            self.rev("HEAD")
        }

        fn rev(&self, name: &str) -> gix::ObjectId {
            let repo = gix::open(&self.root).expect("open");
            repo.rev_parse_single(name).expect("rev").detach()
        }

        fn repo(&self) -> gix::Repository {
            super::super::repo::open(&self.root, false).expect("open")
        }

        /// Every conflict copy under the root, relative, sorted.
        fn copies(&self) -> Vec<String> {
            let mut out = Vec::new();
            let mut stack = vec![self.root.clone()];
            while let Some(dir) = stack.pop() {
                for entry in std::fs::read_dir(&dir).expect("list").flatten() {
                    let path = entry.path();
                    if entry.file_name() == ".git" {
                        continue;
                    }
                    if path.is_dir() {
                        stack.push(path);
                    } else if entry
                        .file_name()
                        .to_string_lossy()
                        .contains(".sync-conflict-")
                    {
                        out.push(
                            path.strip_prefix(&self.root)
                                .expect("under root")
                                .to_string_lossy()
                                .into_owned(),
                        );
                    }
                }
            }
            out.sort();
            out
        }
    }

    const STAMP: &str = "20261018-120000";

    /// A common base, the remote's commit on `theirs`, the phone's on `main`.
    fn diverged() -> Option<(Fixture, gix::ObjectId, gix::ObjectId, gix::ObjectId)> {
        let fx = Fixture::new()?;
        fx.write("both.md", b"base\n");
        fx.write("remote-only-edit.md", b"base\n");
        fx.write("phone-only-edit.md", b"base\n");
        fx.write("phone-deleted.md", b"base\n");
        let base = fx.commit("base");
        fx.git(&["checkout", "-q", "-b", "theirs"]);
        fx.write("both.md", b"remote\n");
        fx.write("remote-only-edit.md", b"remote\n");
        fx.write("remote-new.md", b"remote\n");
        let theirs = fx.commit("remote");
        fx.git(&["checkout", "-q", "main"]);
        fx.write("both.md", b"phone\n");
        fx.write("phone-only-edit.md", b"phone\n");
        fx.write("notes/captured.md", b"captured on the phone\n");
        std::fs::remove_file(fx.root.join("phone-deleted.md")).expect("rm");
        let ours = fx.commit("phone");
        Some((fx, base, theirs, ours))
    }

    #[test]
    fn a_reset_takes_the_remote_and_keeps_the_phone_beside_it() {
        let Some((fx, _base, theirs, ours)) = diverged() else {
            return;
        };
        let repo = fx.repo();
        let interrupt = AtomicBool::new(false);
        let reset = reset_to(&repo, theirs, STAMP, "kalypso", &interrupt).expect("reset");

        assert_eq!(fx.rev("HEAD"), theirs, "the branch is the remote's");
        let saved = reset.saved.as_deref().expect("history kept");
        assert_eq!(saved, format!("{SAVED_PREFIX}{STAMP}"));
        assert_eq!(fx.rev(saved), ours, "the phone's history is reachable");

        // The remote's bytes at every path it holds — nothing it has deleted.
        assert_eq!(fx.read("both.md"), b"remote\n");
        assert_eq!(fx.read("remote-only-edit.md"), b"remote\n");
        assert_eq!(fx.read("remote-new.md"), b"remote\n");
        assert_eq!(
            fx.read("phone-deleted.md"),
            b"base\n",
            "a deletion carries nothing; the remote's file comes back"
        );
        assert_eq!(
            fx.read("phone-only-edit.md"),
            b"base\n",
            "the path is the remote's, which never changed it"
        );

        // The phone's revisions beside them, and its new file in place.
        assert_eq!(
            fx.copies(),
            vec![
                "both.sync-conflict-20261018-120000-kalypso.md".to_owned(),
                "phone-only-edit.sync-conflict-20261018-120000-kalypso.md".to_owned(),
            ]
        );
        assert_eq!(
            fx.read("both.sync-conflict-20261018-120000-kalypso.md"),
            b"phone\n"
        );
        assert_eq!(
            fx.read("phone-only-edit.sync-conflict-20261018-120000-kalypso.md"),
            b"phone\n"
        );
        assert_eq!(fx.read("notes/captured.md"), b"captured on the phone\n");
        assert_eq!(reset.kept, vec![PathBuf::from("notes/captured.md")]);
        assert_eq!(reset.copies.len(), 2);
        assert!(reset.only_saved.is_empty());

        // A path the phone never changed is never copied: the remote's own
        // older revision is not the phone's work.
        assert!(!fx
            .copies()
            .iter()
            .any(|copy| copy.starts_with("remote-only-edit")));

        // What is left for the next pass is new files, nothing else: the index
        // and the tracked worktree are the remote's tip.
        let repo = fx.repo();
        let status = super::super::repo::status_paths(&repo).expect("status");
        assert!(status.modified.is_empty(), "{:?}", status.modified);
        assert!(status.deleted.is_empty(), "{:?}", status.deleted);
        let mut untracked: Vec<String> = status
            .untracked
            .iter()
            .map(|path| path.to_string_lossy().into_owned())
            .collect();
        untracked.sort();
        assert_eq!(
            untracked,
            vec![
                "both.sync-conflict-20261018-120000-kalypso.md".to_owned(),
                "notes/captured.md".to_owned(),
                "phone-only-edit.sync-conflict-20261018-120000-kalypso.md".to_owned(),
            ]
        );
    }

    /// Kalypso's shape: a root commit the phone made before its first clone,
    /// with nothing in common with the remote. Every path of the phone's tree
    /// is the phone's; the ones the remote holds with the same bytes are not
    /// copied, the ones it holds differently are, the rest stay in place.
    #[test]
    fn a_reset_off_a_parentless_root_keeps_every_file_the_phone_made() {
        let Some(fx) = Fixture::new() else {
            return;
        };
        fx.write("README.md", b"drive\n");
        fx.write("notes/index.md", b"remote index\n");
        let theirs = fx.commit("the remote's history");
        fx.git(&["checkout", "-q", "--orphan", "seeded"]);
        fx.git(&["rm", "-rq", "--cached", "."]);
        std::fs::remove_file(fx.root.join("notes/index.md")).expect("rm");
        fx.write("notes/index.md", b"seeded index\n");
        fx.write("notes/inbox.md", b"seeded inbox\n");
        let ours = fx.commit("seed");
        fx.git(&["branch", "-qf", "main", "seeded"]);
        fx.git(&["checkout", "-q", "main"]);
        fx.git(&["branch", "-qD", "seeded"]);
        assert_eq!(fx.rev("HEAD"), ours);

        let repo = fx.repo();
        let reset =
            reset_to(&repo, theirs, STAMP, "kalypso", &AtomicBool::new(false)).expect("reset");
        assert_eq!(fx.rev("HEAD"), theirs);
        assert_eq!(fx.read("README.md"), b"drive\n", "same bytes: no copy");
        assert_eq!(fx.read("notes/index.md"), b"remote index\n");
        assert_eq!(
            fx.read("notes/index.sync-conflict-20261018-120000-kalypso.md"),
            b"seeded index\n"
        );
        assert_eq!(fx.read("notes/inbox.md"), b"seeded inbox\n");
        assert_eq!(
            fx.copies(),
            vec!["notes/index.sync-conflict-20261018-120000-kalypso.md".to_owned()]
        );
        assert_eq!(reset.kept, vec![PathBuf::from("notes/inbox.md")]);
    }

    /// A local change on a path the reset would write refuses it — git's
    /// refusal, a fast-forward's — and leaves the phone exactly as it was:
    /// the branch, the edit, and no copy behind.
    #[test]
    fn a_reset_over_an_uncommitted_edit_refuses_and_leaves_no_copy() {
        let Some((fx, _base, theirs, ours)) = diverged() else {
            return;
        };
        fx.write("both.md", b"typed since the commit\n");
        let repo = fx.repo();
        let err = reset_to(&repo, theirs, STAMP, "kalypso", &AtomicBool::new(false))
            .expect_err("an edit in the way refuses");
        assert!(err.to_string().contains("local change"), "{err}");
        assert_eq!(fx.rev("HEAD"), ours, "the branch did not move");
        assert_eq!(fx.read("both.md"), b"typed since the commit\n");
        assert!(fx.copies().is_empty(), "{:?}", fx.copies());
    }

    /// Two resets in one second keep two rescues.
    #[test]
    fn a_second_reset_in_the_same_second_keeps_its_own_ref() {
        let Some((fx, base, theirs, ours)) = diverged() else {
            return;
        };
        let repo = fx.repo();
        let first =
            reset_to(&repo, theirs, STAMP, "kalypso", &AtomicBool::new(false)).expect("reset");
        // The phone diverges again off the remote's tip, from `base`'s side.
        fx.git(&["reset", "-q", "--hard", &base.to_string()]);
        fx.git(&["clean", "-qfd"]);
        fx.write("again.md", b"again\n");
        let again = fx.commit("again");
        let repo = fx.repo();
        let second =
            reset_to(&repo, theirs, STAMP, "kalypso", &AtomicBool::new(false)).expect("reset");
        assert_eq!(first.saved.as_deref().map(|s| fx.rev(s)), Some(ours));
        assert_eq!(
            second.saved.as_deref(),
            Some(format!("{SAVED_PREFIX}{STAMP}-2").as_str())
        );
        assert_eq!(fx.rev(&format!("{SAVED_PREFIX}{STAMP}-2")), again);
    }

    /// Already the remote's: nothing moves and nothing is saved.
    #[test]
    fn a_copy_already_at_the_remote_resets_to_nothing() {
        let Some(fx) = Fixture::new() else {
            return;
        };
        fx.write("a.md", b"a\n");
        let tip = fx.commit("tip");
        let repo = fx.repo();
        let reset = reset_to(&repo, tip, STAMP, "kalypso", &AtomicBool::new(false)).expect("reset");
        assert_eq!(reset, Reset::default());
        let refs = gix::open(&fx.root).expect("open");
        assert!(refs
            .references()
            .expect("refs")
            .prefixed(SAVED_PREFIX)
            .expect("prefixed")
            .next()
            .is_none());
    }
}
