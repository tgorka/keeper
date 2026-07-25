//! Repository lifecycle: open, clone, config enforcement, status
//! (Story 24.1, AD-41 / AD-47 / AD-48).
//!
//! Everything here is gitoxide. Two of these functions exist purely to close a
//! silent-failure mode that would otherwise corrupt a user's data, and both are
//! worth reading before touching:
//!
//! * [`open`] can be asked for `Trust::Full`. gitoxide derives trust from
//!   **directory ownership**, and a repository written by another machine — the
//!   normal case on a pendrive (AD-48) — comes back as `Trust::Reduced`. Under
//!   reduced trust gix **silently drops every repo-local `filter.*` driver**,
//!   with no error and no warning, so the LFS clean filter simply never runs
//!   and a multi-gigabyte file is committed raw into the object database
//!   (AD-46).
//! * [`enforce_local_config`] forces `index.sparse=false`. `gix::status`
//!   **hard-fails** with `TreeIndexDiff(IsSparse)` on a true sparse index, so a
//!   repository that plain `git` created with `index.sparse=true` is unusable
//!   to this engine until the key is set. A non-sparse index carrying
//!   `SKIP_WORKTREE` flags is fully understood, which is why AD-47 pairs cone
//!   sparse-checkout with a non-sparse index rather than avoiding sparsity.

use std::{
    num::NonZeroU32,
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
};

use crate::error::{Result, SyncError};

/// Open a managed repository.
///
/// Pass `trust_full` for a repository the engine put there itself — including
/// one on removable media owned by another uid. See the module docs: without it
/// gitoxide discards repo-local filter configuration without saying so.
pub fn open(path: &Path, trust_full: bool) -> Result<gix::Repository> {
    let mut options = gix::open::Options::default();
    if trust_full {
        options = options.with(gix::sec::Trust::Full);
    }
    gix::open_opts(path, options).map_err(|err| SyncError::Git(format!("open failed: {err}")))
}

/// Clone `url` into `dest` on `branch`.
///
/// `index.sparse=false` is applied as an in-memory override for the clone
/// itself; [`enforce_local_config`] must still be called afterwards to make it
/// durable, because the override does not reach `.git/config`.
///
/// Blocking: gitoxide's HTTP transport has no async path, so callers on a tokio
/// runtime must wrap this in `spawn_blocking`.
pub fn clone(
    url: &str,
    dest: &Path,
    branch: &str,
    shallow_depth: Option<NonZeroU32>,
    interrupt: &AtomicBool,
) -> Result<gix::Repository> {
    let mut prepare = gix::prepare_clone(url, dest)
        .map_err(|err| SyncError::Config(format!("invalid remote URL: {err}")))?
        .with_in_memory_config_overrides(["index.sparse=false"])
        .with_ref_name(Some(branch))
        .map_err(|err| SyncError::Config(format!("invalid branch name {branch:?}: {err}")))?;

    if let Some(depth) = shallow_depth {
        prepare = prepare.with_shallow(gix::remote::fetch::Shallow::DepthAtRemote(depth));
    }

    let (mut checkout, _outcome) = prepare
        .fetch_then_checkout(gix::progress::Discard, interrupt)
        .map_err(|err| {
            cancelled_or(interrupt, || SyncError::Git(format!("clone failed: {err}")))
        })?;

    let (repo, _outcome) = checkout
        .main_worktree(gix::progress::Discard, interrupt)
        .map_err(|err| {
            cancelled_or(interrupt, || {
                SyncError::Git(format!("checkout failed: {err}"))
            })
        })?;

    enforce_local_config(&repo)?;
    Ok(repo)
}

/// Write `index.sparse=false` into the repository's own `.git/config`.
///
/// Mandatory for every managed repository — see the module docs for the
/// hard-failure this prevents. Idempotent: an existing value is overwritten
/// rather than appended, so repeated calls do not grow the file.
pub fn enforce_local_config(repo: &gix::Repository) -> Result<()> {
    enforce_local_config_with_filter(repo, None)
}

/// As [`enforce_local_config`], additionally registering keeper as the `lfs`
/// clean/smudge filter when `filter_program` is given.
///
/// Registering the filter is what lets a human use plain `git` inside a synced
/// folder. Without it, the blob is a pointer while the worktree holds the real
/// bytes, so the moment git re-reads content — which it does whenever its stat
/// cache misses — it reports every LFS-tracked file as modified. keeper itself
/// tolerates that, but nobody running `git status` by hand should have to.
///
/// Single-invocation `clean`/`smudge` rather than the long-running
/// `filter.lfs.process` protocol: both git and gitoxide support it, it is far
/// less machinery, and the per-call cost only lands when git's stat cache
/// already missed.
///
/// `required` is deliberately left false. A worktree whose keeper binary has
/// moved must still be checkout-able — it would just get pointers, which is
/// recoverable, where a required filter would hard-fail every git command.
pub fn enforce_local_config_with_filter(
    repo: &gix::Repository,
    filter_program: Option<&Path>,
) -> Result<()> {
    let path = repo.git_dir().join("config");
    let mut config =
        gix::config::File::from_path_no_includes(path.clone(), gix::config::Source::Local)
            .map_err(|err| SyncError::Git(format!("could not read {}: {err}", path.display())))?;
    config
        .set_raw_value("index.sparse", "false")
        .map_err(|err| SyncError::Git(format!("could not set index.sparse: {err}")))?;

    if let Some(program) = filter_program {
        let workdir = workdir(repo)?;
        // `%f` is git's placeholder for the path being filtered. The program
        // path is quoted because a user's install directory may contain spaces,
        // and git splits this value on whitespace.
        let quoted = program.display().to_string();
        let clean = format!("\"{quoted}\" lfs clean --repo \"{}\" %f", workdir.display());
        let smudge = format!(
            "\"{quoted}\" lfs smudge --repo \"{}\" %f",
            workdir.display()
        );
        config
            .set_raw_value("filter.lfs.clean", clean.as_str())
            .map_err(|err| SyncError::Git(format!("could not set filter.lfs.clean: {err}")))?;
        config
            .set_raw_value("filter.lfs.smudge", smudge.as_str())
            .map_err(|err| SyncError::Git(format!("could not set filter.lfs.smudge: {err}")))?;
        config
            .set_raw_value("filter.lfs.required", "false")
            .map_err(|err| SyncError::Git(format!("could not set filter.lfs.required: {err}")))?;
    }

    let parent = path
        .parent()
        .ok_or_else(|| SyncError::Config(format!("{} has no parent", path.display())))?;
    // Written tmp-then-rename: a torn `.git/config` is a bricked repository,
    // and this runs on a pendrive that can be unplugged mid-write (AD-48).
    let mut staged = tempfile::NamedTempFile::new_in(parent)
        .map_err(|source| SyncError::io("stage .git/config", parent.to_path_buf(), source))?;
    config
        .write_to(&mut staged)
        .map_err(|source| SyncError::io("write .git/config", path.clone(), source))?;
    // A temp file is created 0600; keep whatever mode the repository already
    // used so a shared-group checkout does not silently become private.
    if let Ok(metadata) = std::fs::metadata(&path) {
        let _ = std::fs::set_permissions(staged.path(), metadata.permissions());
    }
    staged
        .persist(&path)
        .map_err(|err| SyncError::io("replace .git/config", path.clone(), err.error))?;

    Ok(())
}

/// Repository-relative paths that differ between `HEAD`, the index and the
/// working tree.
///
/// Each vector is sorted and deduplicated so a caller can diff two snapshots
/// and so tests are order-independent.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct RepoStatus {
    /// Tracked paths that did not exist in `HEAD`.
    pub added: Vec<PathBuf>,
    /// Tracked paths whose content or mode changed.
    pub modified: Vec<PathBuf>,
    /// Tracked paths that are gone.
    pub deleted: Vec<PathBuf>,
    /// Paths on disk that git does not track.
    pub untracked: Vec<PathBuf>,
}

impl RepoStatus {
    /// Whether anything at all differs.
    pub fn is_empty(&self) -> bool {
        self.added.is_empty()
            && self.modified.is_empty()
            && self.deleted.is_empty()
            && self.untracked.is_empty()
    }
}

/// Classify everything `gix::status` reports.
///
/// Both halves of the status are consumed: `HEAD`-to-index changes (already
/// staged) and index-to-worktree changes (not yet staged). A path can therefore
/// legitimately appear in two buckets — staged as added, then edited again —
/// and that is reported rather than collapsed.
pub fn status_paths(repo: &gix::Repository) -> Result<RepoStatus> {
    use gix::status::{index_worktree::Item as WorktreeItem, plumbing::index_as_worktree, Item};

    let platform = repo
        .status(gix::progress::Discard)
        .map_err(|err| SyncError::Git(format!("status failed: {err}")))?;
    let iter = platform
        .into_iter(None)
        .map_err(|err| SyncError::Git(format!("status failed: {err}")))?;

    let mut out = RepoStatus::default();
    for item in iter {
        let item = item.map_err(|err| SyncError::Git(format!("status failed: {err}")))?;
        match item {
            Item::TreeIndex(change) => match change {
                gix::diff::index::Change::Addition { location, .. } => {
                    out.added.push(to_path(location.as_ref()));
                }
                gix::diff::index::Change::Deletion { location, .. } => {
                    out.deleted.push(to_path(location.as_ref()));
                }
                gix::diff::index::Change::Modification { location, .. } => {
                    out.modified.push(to_path(location.as_ref()));
                }
                // A rename is a deletion at the source fused with an addition
                // at the destination; keeping both keeps the two vectors a
                // faithful description of what the tree has to become.
                gix::diff::index::Change::Rewrite {
                    source_location,
                    location,
                    ..
                } => {
                    out.deleted.push(to_path(source_location.as_ref()));
                    out.added.push(to_path(location.as_ref()));
                }
            },
            Item::IndexWorktree(WorktreeItem::Modification {
                rela_path, status, ..
            }) => match status {
                index_as_worktree::EntryStatus::Change(index_as_worktree::Change::Removed) => {
                    out.deleted.push(to_path(rela_path.as_ref()));
                }
                index_as_worktree::EntryStatus::Change(_) => {
                    out.modified.push(to_path(rela_path.as_ref()));
                }
                // A conflicted entry is a divergence the merge machinery never
                // produces here (AD-43 makes conflict copies instead), and
                // `NeedsUpdate` / `IntentToAdd` mean the content is unchanged.
                _ => {}
            },
            Item::IndexWorktree(WorktreeItem::DirectoryContents { entry, .. }) => {
                if entry.status == gix::dir::entry::Status::Untracked {
                    out.untracked.push(to_path(entry.rela_path.as_ref()));
                }
            }
            Item::IndexWorktree(WorktreeItem::Rewrite {
                source,
                dirwalk_entry,
                ..
            }) => {
                out.deleted.push(to_path(source.rela_path()));
                out.added.push(to_path(dirwalk_entry.rela_path.as_ref()));
            }
        }
    }

    for bucket in [
        &mut out.added,
        &mut out.modified,
        &mut out.deleted,
        &mut out.untracked,
    ] {
        bucket.sort();
        bucket.dedup();
    }
    Ok(out)
}

/// The commit `HEAD` resolves to, or `None` on an unborn branch.
///
/// A freshly initialized repository with no commits is an ordinary state, not
/// an error — every profile starts there.
pub fn head_commit_id(repo: &gix::Repository) -> Result<Option<gix::hash::ObjectId>> {
    let mut head = repo
        .head()
        .map_err(|err| SyncError::Git(format!("could not read HEAD: {err}")))?;
    let id = head
        .try_peel_to_id()
        .map_err(|err| SyncError::Git(format!("could not peel HEAD: {err}")))?;
    Ok(id.map(gix::Id::detach))
}

/// Whether the index or the working tree differs from `HEAD`.
///
/// Untracked files do **not** make a repository dirty, matching `git status`.
pub fn is_dirty(repo: &gix::Repository) -> Result<bool> {
    repo.is_dirty()
        .map_err(|err| SyncError::Git(format!("dirty check failed: {err}")))
}

/// The working tree root.
///
/// A bare repository has none; that is a configuration error rather than a
/// panic, because a user can point a profile at one by mistake.
pub fn workdir(repo: &gix::Repository) -> Result<PathBuf> {
    repo.workdir().map(Path::to_path_buf).ok_or_else(|| {
        SyncError::Config(format!(
            "repository at {} is bare and has no working tree to synchronize",
            repo.git_dir().display()
        ))
    })
}

/// Repository-relative git path to a native path.
fn to_path(rela_path: &gix::bstr::BStr) -> PathBuf {
    gix::path::from_bstr(rela_path).into_owned()
}

/// Prefer `Cancelled` when the caller pulled the plug.
///
/// gitoxide surfaces an interruption as an ordinary transport error, and a
/// user-requested stop must never be reported as a failure (it would otherwise
/// be retried with backoff and shown as a warning).
fn cancelled_or(interrupt: &AtomicBool, otherwise: impl FnOnce() -> SyncError) -> SyncError {
    if interrupt.load(Ordering::Relaxed) {
        SyncError::Cancelled
    } else {
        otherwise()
    }
}

/// Every path the index tracks, repository-relative.
///
/// Used to find checked-out LFS pointers that still need materializing: only a
/// tracked path can hold one, so this bounds that scan to the index rather than
/// walking the whole worktree.
pub fn tracked_paths(repo: &gix::Repository) -> Result<Vec<PathBuf>> {
    let index = repo
        .index_or_empty()
        .map_err(|err| SyncError::Git(format!("could not read the index: {err}")))?;
    Ok(index
        .entries()
        .iter()
        .map(|entry| PathBuf::from(entry.path(&index).to_string()))
        .collect())
}

/// Re-stat `paths` and write the refreshed index.
///
/// Materializing an LFS object replaces a ~130-byte pointer with the real file,
/// so the entry's cached stat no longer describes what is on disk and `status`
/// would report every one of them as modified. Refreshing the stat — while
/// leaving the pointer blob as the entry's object — restores the invariant the
/// whole design rests on: pointer blob, worktree stat, clean status.
pub fn refresh_index_stat(repo: &gix::Repository, paths: &[PathBuf]) -> Result<()> {
    if paths.is_empty() {
        return Ok(());
    }
    let workdir = workdir(repo)?;
    let shared = repo
        .index_or_empty()
        .map_err(|err| SyncError::Git(format!("could not read the index: {err}")))?;
    let mut index: gix::index::File = (**shared).clone();

    let wanted: std::collections::BTreeSet<String> = paths
        .iter()
        .map(|p| p.to_string_lossy().replace('\\', "/"))
        .collect();

    let mut touched = false;
    for idx in 0..index.entries().len() {
        let key = {
            let state = &*index;
            let entry = &state.entries()[idx];
            entry.path(state).to_string()
        };
        if !wanted.contains(&key) {
            continue;
        }
        let absolute = workdir.join(&key);
        let Ok(metadata) = gix::index::fs::Metadata::from_path_no_follow(&absolute) else {
            continue;
        };
        // A pre-epoch timestamp is the only failure mode, and an all-zero stat
        // simply makes the entry racily clean so the next status re-reads it.
        if let Ok(stat) = gix::index::entry::Stat::from_fs(&metadata) {
            index.entries_mut()[idx].stat = stat;
            touched = true;
        }
    }
    if !touched {
        return Ok(());
    }
    index
        .write(gix::index::write::Options::default())
        .map_err(|err| SyncError::Git(format!("could not write the index: {err}")))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        git::commit::{stage_and_commit, StagedChange},
        provenance::{Provenance, SyncSource},
    };

    fn signature() -> gix::actor::Signature {
        gix::actor::Signature {
            name: "Keeper".into(),
            email: "sync@01ABC.keeper.invalid".into(),
            time: gix::date::Time::new(1_700_000_000, 0),
        }
    }

    fn provenance() -> Provenance {
        Provenance::new("fixture", "test-box", "01ABC", "localhost", SyncSource::Cli)
    }

    /// A repository with `a.txt` and `b.txt` committed.
    fn repo_with_two_files() -> (tempfile::TempDir, gix::Repository) {
        let dir = tempfile::tempdir().expect("tempdir");
        let repo = gix::init(dir.path()).expect("init");
        std::fs::write(dir.path().join("a.txt"), "alpha").expect("write a");
        std::fs::write(dir.path().join("b.txt"), "beta").expect("write b");
        let changes = StagedChange {
            added: vec![PathBuf::from("a.txt"), PathBuf::from("b.txt")],
            ..StagedChange::default()
        };
        stage_and_commit(
            &repo,
            &changes,
            &provenance(),
            "fixture",
            &signature(),
            &std::collections::BTreeMap::new(),
        )
        .expect("commit")
        .expect("a non-empty commit");
        // Re-open so the index snapshot is read fresh from disk, exactly as a
        // supervisor would on its next pass.
        let repo = open(dir.path(), true).expect("reopen");
        (dir, repo)
    }

    #[test]
    fn enforce_local_config_writes_the_key_that_keeps_status_working() {
        let dir = tempfile::tempdir().expect("tempdir");
        let repo = gix::init(dir.path()).expect("init");

        enforce_local_config(&repo).expect("enforce");

        // Re-open so the assertion goes through the same path gix will use.
        let reopened = open(dir.path(), true).expect("reopen");
        assert_eq!(
            reopened.config_snapshot().boolean("index.sparse"),
            Some(false),
            "without this, gix::status hard-fails on a sparse index"
        );
    }

    #[test]
    fn enforce_local_config_is_idempotent() {
        let dir = tempfile::tempdir().expect("tempdir");
        let repo = gix::init(dir.path()).expect("init");
        enforce_local_config(&repo).expect("first");
        enforce_local_config(&repo).expect("second");

        let text = std::fs::read_to_string(dir.path().join(".git/config")).expect("read config");
        assert_eq!(
            text.matches("sparse").count(),
            1,
            "a repeated call must overwrite, not append: {text}"
        );
    }

    #[test]
    fn open_with_trust_full_reports_full_trust() {
        // Load-bearing for removable media: under the derived `Trust::Reduced`
        // gix drops repo-local `filter.*` and the LFS clean filter never runs.
        let dir = tempfile::tempdir().expect("tempdir");
        gix::init(dir.path()).expect("init");

        let repo = open(dir.path(), true).expect("open");
        assert_eq!(repo.git_dir_trust(), gix::sec::Trust::Full);
    }

    #[test]
    fn status_reports_untracked_files_and_nothing_else() {
        let dir = tempfile::tempdir().expect("tempdir");
        let repo = gix::init(dir.path()).expect("init");
        std::fs::write(dir.path().join("new.txt"), "hello").expect("write");

        let status = status_paths(&repo).expect("status");
        assert_eq!(status.untracked, [PathBuf::from("new.txt")]);
        assert!(status.added.is_empty());
        assert!(status.modified.is_empty());
        assert!(status.deleted.is_empty());
    }

    #[test]
    fn status_classifies_a_modification_and_a_deletion_over_a_committed_fixture() {
        let (dir, repo) = repo_with_two_files();
        // Different length as well as different content, so the change is
        // visible from `lstat` alone and cannot be missed as racily clean.
        std::fs::write(dir.path().join("a.txt"), "alpha-changed").expect("modify");
        std::fs::remove_file(dir.path().join("b.txt")).expect("delete");

        let status = status_paths(&repo).expect("status");
        assert_eq!(status.modified, [PathBuf::from("a.txt")]);
        assert_eq!(status.deleted, [PathBuf::from("b.txt")]);
        assert!(status.untracked.is_empty());
    }

    #[test]
    fn a_clean_committed_repository_reports_no_changes() {
        let (_dir, repo) = repo_with_two_files();
        let status = status_paths(&repo).expect("status");
        assert!(status.is_empty(), "unexpected changes: {status:?}");
        assert!(!is_dirty(&repo).expect("dirty check"));
    }

    #[test]
    fn head_commit_id_is_none_on_an_unborn_branch() {
        let dir = tempfile::tempdir().expect("tempdir");
        let repo = gix::init(dir.path()).expect("init");
        assert_eq!(head_commit_id(&repo).expect("head"), None);
    }

    #[test]
    fn head_commit_id_follows_a_commit() {
        let (_dir, repo) = repo_with_two_files();
        assert!(head_commit_id(&repo).expect("head").is_some());
    }

    #[test]
    fn workdir_on_a_bare_repository_errors_instead_of_panicking() {
        let dir = tempfile::tempdir().expect("tempdir");
        let repo = gix::init_bare(dir.path()).expect("init bare");

        let err = workdir(&repo).expect_err("a bare repo has no working tree");
        assert_eq!(err.code(), "config");
    }

    #[test]
    fn cloning_a_local_fixture_checks_out_and_pins_the_sparse_index_off() {
        // No network: the source is a bare repository in a tempdir, which
        // still exercises the real fetch/checkout path end to end.
        let source_dir = tempfile::tempdir().expect("tempdir");
        let source = gix::init_bare(source_dir.path()).expect("init bare");
        let blob = source.write_blob(b"alpha").expect("blob").detach();
        let tree = gix::objs::Tree {
            entries: vec![gix::objs::tree::Entry {
                mode: gix::objs::tree::EntryKind::Blob.into(),
                filename: "a.txt".into(),
                oid: blob,
            }],
        };
        let tree = source.write_object(&tree).expect("tree").detach();
        let mut buf = gix::date::parse::TimeBuf::default();
        let author = signature();
        let author = author.to_ref(&mut buf);
        source
            .commit_as(
                author,
                author,
                "HEAD",
                "root",
                tree,
                Vec::<gix::hash::ObjectId>::new(),
            )
            .expect("commit");
        let branch = source
            .head_name()
            .expect("head name")
            .expect("a born branch")
            .shorten()
            .to_string();

        let dest_dir = tempfile::tempdir().expect("tempdir");
        let dest = dest_dir.path().join("clone");
        let interrupt = AtomicBool::new(false);
        clone(
            &source_dir.path().to_string_lossy(),
            &dest,
            &branch,
            None,
            &interrupt,
        )
        .expect("clone from a local bare repository");

        assert_eq!(
            std::fs::read_to_string(dest.join("a.txt")).expect("checked out"),
            "alpha"
        );
        // Re-opened from disk on purpose: the clone-time override is only
        // in-memory, so reading the live handle would pass even if
        // `enforce_local_config` had written nothing.
        let reopened = open(&dest, true).expect("reopen");
        assert_eq!(
            reopened.config_snapshot().boolean("index.sparse"),
            Some(false),
            "clone must leave the sparse index disabled in .git/config"
        );
    }
}
