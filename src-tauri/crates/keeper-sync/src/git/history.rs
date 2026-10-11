//! The phone's history reads (Epic 66, Story 66.4, AD-198).
//!
//! The desktop answers "who changed this note", "what did it say at that
//! revision" and "what changed between these two" with `git log`, `git show`
//! and `git diff` — reads, and cheap ones, so the shell never needed an engine
//! API for them. A phone spawns nothing, so the same four questions are
//! answered here in-process, over the same repository, with gitoxide:
//!
//! * [`file_log`] — the commits that touched one path, newest first;
//! * [`recent_commits`] — the last N commits and which paths under a prefix
//!   each touched, which is what the unread projection is built from;
//! * [`blob_at`] — one path's bytes as of one revision;
//! * [`unified_diff`] — the `@@` hunks between two revisions, or a revision
//!   and the working tree, in the format `git diff --unified=3` prints;
//! * [`dirty_paths`] — the paths under a prefix whose bytes differ from `HEAD`.
//! * [`changed_between`] and [`holds_commit`] — what a pushed range touched,
//!   and whether a commit a peer named is here yet: what a doorbell is rung
//!   from and answered with, on every platform.
//! * [`files_at`] and [`path_changes`] — what one pinned commit holds under
//!   a folder, and which commits of its history changed a path, merges read
//!   as git simplifies them: what the weekly curator reads a drive at.
//!
//! One deliberate difference from the desktop: [`file_log`] does not follow
//! renames (`git log --follow`). A note's identity is its ULID and survives a
//! rename already (FR-97); what stops at the rename is the list of older
//! revisions under the previous filename, and the phone says nothing about
//! them rather than guessing. A rename-detecting walk is `git`'s heuristic
//! over every commit's whole diff, and the phone reads history on a battery.
//!
//! Every function takes the repository's path and opens it read-only for the
//! call: history is a read, and holding a `gix::Repository` across the notes
//! registry's lifetime would pin an object store the reconciler never needs.

use std::{collections::HashSet, path::Path};

use gix::bstr::ByteSlice as _;

use super::repo::{open_read_only, status_paths_for};
use crate::error::{Result, SyncError};

/// One commit, as the desktop's `--format=%H%x1f%ct%x1f%B` printed it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileRevision {
    /// The full hex object id — the string the history panel and the restore
    /// verb hand back, so both name one object.
    pub id: String,
    /// Committer time in whole seconds since the epoch (`%ct`).
    pub committed_secs: i64,
    /// The whole message, subject line and trailers included (`%B`).
    pub message: String,
}

impl FileRevision {
    /// The subject: the first line of the message (`%s`).
    pub fn subject(&self) -> &str {
        self.message.lines().next().unwrap_or("").trim()
    }
}

/// One commit and the paths under the asked-for prefix it touched.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TouchedCommit {
    pub revision: FileRevision,
    /// Repository-relative, `/`-separated, prefix included — exactly what
    /// `git log --name-only` prints.
    pub paths: Vec<String>,
}

/// Open for reading, with the object cache a walk that decodes every commit's
/// tree wants.
fn open(repo_path: &Path) -> Result<gix::Repository> {
    let mut repo = open_read_only(repo_path, true)?;
    repo.object_cache_size_if_unset(4 * 1024 * 1024);
    Ok(repo)
}

fn walk_error(err: &dyn std::error::Error) -> SyncError {
    SyncError::Git(format!(
        "could not read the local history: {}",
        super::fetch::flatten(err)
    ))
}

/// `HEAD`'s commit, or `None` on an unborn branch — a repository that has
/// never committed has an honest empty history, not an error (AD-63).
fn head(repo: &gix::Repository) -> Result<Option<gix::ObjectId>> {
    super::repo::head_commit_id(repo)
}

/// The walk every reader here runs: from `HEAD`, newest first.
fn walk(repo: &gix::Repository) -> Result<Option<gix::revision::Walk<'_>>> {
    let Some(tip) = head(repo)? else {
        return Ok(None);
    };
    repo.rev_walk([tip])
        .sorting(gix::revision::walk::Sorting::ByCommitTime(
            gix::traverse::commit::simple::CommitTimeOrder::NewestFirst,
        ))
        .all()
        .map(Some)
        .map_err(|err| walk_error(&err))
}

fn revision_of(commit: &gix::Commit<'_>) -> Result<FileRevision> {
    let committed_secs = commit.time().map_err(|err| walk_error(&err))?.seconds;
    let message = commit
        .message_raw()
        .map_err(|err| walk_error(&err))?
        .to_str_lossy()
        .into_owned();
    Ok(FileRevision {
        id: commit.id().to_hex().to_string(),
        committed_secs,
        message,
    })
}

/// The blob a tree holds at `rel`, or `None` where the path is not in it.
fn entry_at(tree: &gix::Tree<'_>, rel: &str) -> Result<Option<gix::ObjectId>> {
    tree.lookup_entry_by_path(rel)
        .map(|entry| entry.map(|entry| entry.object_id()))
        .map_err(|err| walk_error(&err))
}

/// The first parent's tree, or the empty tree for a root commit.
fn parent_tree<'repo>(
    repo: &'repo gix::Repository,
    commit: &gix::Commit<'repo>,
) -> Result<gix::Tree<'repo>> {
    let Some(parent) = commit.parent_ids().next() else {
        return Ok(repo.empty_tree());
    };
    let object = parent.object().map_err(|err| walk_error(&err))?;
    let commit = object.try_into_commit().map_err(|err| walk_error(&err))?;
    commit.tree().map_err(|err| walk_error(&err))
}

/// The commits that changed `rel` — added it, rewrote it, or removed it —
/// newest first, at most `limit` of them.
///
/// A commit counts when the blob at `rel` differs from its first parent's,
/// which is what `git log -- <path>` reports on a linear history. A merge
/// commit that carries another side's change to the path is listed once, as
/// git lists it.
pub fn file_log(repo_path: &Path, rel: &str, limit: usize) -> Result<Vec<FileRevision>> {
    let repo = open(repo_path)?;
    let mut out = Vec::new();
    let Some(walk) = walk(&repo)? else {
        return Ok(out);
    };
    for info in walk {
        if out.len() >= limit {
            break;
        }
        let info = info.map_err(|err| walk_error(&err))?;
        let commit = info.object().map_err(|err| walk_error(&err))?;
        let here = entry_at(&commit.tree().map_err(|err| walk_error(&err))?, rel)?;
        let before = entry_at(&parent_tree(&repo, &commit)?, rel)?;
        if here != before {
            out.push(revision_of(&commit)?);
        }
    }
    Ok(out)
}

/// One file of a commit's tree, as [`files_at`] lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TreeFile {
    /// Repository-relative, `/`-separated.
    pub path: String,
    /// Its git object id, hex.
    pub id: String,
    /// Its size in bytes, read from the object's header.
    pub size: u64,
    /// A plain or executable file: not a link, not a submodule.
    pub regular: bool,
}

/// The commit `rev` names (anything `git rev-parse` accepts); an error when
/// this copy does not hold it, unlike [`blob_at`], because a reader pinned
/// to one revision must not read another.
fn pinned<'repo>(repo: &'repo gix::Repository, rev: &str) -> Result<gix::Commit<'repo>> {
    let missing = || SyncError::Git(format!("this copy does not hold the commit {rev}"));
    repo.rev_parse_single(rev)
        .map_err(|_| missing())?
        .object()
        .map_err(|_| missing())?
        .peel_to_kind(gix::object::Kind::Commit)
        .map_err(|_| missing())
        .map(gix::Object::into_commit)
}

/// Every file under the folder `prefix` (repository-relative, no trailing
/// `/`) in the commit `rev` names, sorted by path; empty when the commit
/// holds no folder there. Read from git's objects alone: nothing on the
/// disk is looked at, so no link there leads the listing anywhere.
pub fn files_at(repo_path: &Path, rev: &str, prefix: &str) -> Result<Vec<TreeFile>> {
    let repo = open(repo_path)?;
    let tree = pinned(&repo, rev)?.tree().map_err(|err| walk_error(&err))?;
    let Some(entry) = tree
        .lookup_entry_by_path(prefix)
        .map_err(|err| walk_error(&err))?
    else {
        return Ok(Vec::new());
    };
    if !entry.mode().is_tree() {
        return Ok(Vec::new());
    }
    let folder = entry.object().map_err(|err| walk_error(&err))?.into_tree();
    let mut recorder = gix::traverse::tree::Recorder::default();
    folder
        .traverse()
        .breadthfirst(&mut recorder)
        .map_err(|err| walk_error(&err))?;
    let mut out = Vec::new();
    for record in recorder.records {
        if record.mode.is_tree() {
            continue;
        }
        let size = if record.mode.is_commit() {
            0
        } else {
            repo.find_header(record.oid)
                .map_err(|err| walk_error(&err))?
                .size()
        };
        out.push(TreeFile {
            path: format!("{prefix}/{}", record.filepath.to_str_lossy()),
            id: record.oid.to_hex().to_string(),
            size,
            regular: record.mode.is_blob(),
        });
    }
    out.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(out)
}

/// The commits of `rev`'s history that changed `rel` — a file or a folder —
/// newest first, at most `limit` of them.
///
/// Git's history simplification for a path: a commit that holds at `rel`
/// what one of its parents holds there (TREESAME) is not a change, and the
/// walk follows that one parent only — so a merge that kept one side's
/// version never lists, nor walks into, the history it discarded, and a
/// merge that took a side's change lists the side's own commit that made
/// it. A commit differing from every parent (a root commit's parent is the
/// empty tree) is a change, and every parent is walked.
pub fn path_changes(
    repo_path: &Path,
    rev: &str,
    rel: &str,
    limit: usize,
) -> Result<Vec<FileRevision>> {
    let repo = open(repo_path)?;
    let commit_of = |id: gix::ObjectId| -> Result<gix::Commit<'_>> {
        repo.find_object(id)
            .map_err(|err| walk_error(&err))?
            .try_into_commit()
            .map_err(|err| walk_error(&err))
    };
    let time_of = |commit: &gix::Commit<'_>| -> Result<i64> {
        Ok(commit.time().map_err(|err| walk_error(&err))?.seconds)
    };
    let tip = pinned(&repo, rev)?;
    let mut seen = HashSet::from([tip.id]);
    let mut queue = std::collections::BinaryHeap::from([(time_of(&tip)?, tip.id)]);
    let mut out = Vec::new();
    while let Some((_, id)) = queue.pop() {
        if out.len() >= limit {
            break;
        }
        let commit = commit_of(id)?;
        let here = entry_at(&commit.tree().map_err(|err| walk_error(&err))?, rel)?;
        let parents: Vec<gix::ObjectId> = commit.parent_ids().map(|id| id.detach()).collect();
        let mut differing = Vec::with_capacity(parents.len());
        let mut same = None;
        for parent in parents {
            let parent = commit_of(parent)?;
            let there = entry_at(&parent.tree().map_err(|err| walk_error(&err))?, rel)?;
            if there == here {
                same = Some(parent);
                break;
            }
            differing.push(parent);
        }
        let follow = match same {
            Some(parent) => vec![parent],
            None => {
                if here.is_some() || !differing.is_empty() {
                    out.push(revision_of(&commit)?);
                }
                differing
            }
        };
        for parent in follow {
            if seen.insert(parent.id) {
                queue.push((time_of(&parent)?, parent.id));
            }
        }
    }
    Ok(out)
}

/// The last `limit` commits, newest first, each with the paths under `prefix`
/// it touched. A commit that touched nothing under the prefix is still
/// listed, with an empty path list, so `limit` counts commits and not hits —
/// the same window `git log -n<limit> --name-only -- <prefix>` bounds.
///
/// `prefix` is repository-relative with a trailing `/`, or empty for the
/// whole tree.
pub fn recent_commits(repo_path: &Path, prefix: &str, limit: usize) -> Result<Vec<TouchedCommit>> {
    let repo = open(repo_path)?;
    let mut out = Vec::new();
    let Some(walk) = walk(&repo)? else {
        return Ok(out);
    };
    for info in walk.take(limit) {
        let info = info.map_err(|err| walk_error(&err))?;
        let commit = info.object().map_err(|err| walk_error(&err))?;
        let tree = commit.tree().map_err(|err| walk_error(&err))?;
        let parent = parent_tree(&repo, &commit)?;
        let mut paths = Vec::new();
        parent
            .changes()
            .map_err(|err| walk_error(&err))?
            .options(|options| {
                options.track_path();
                // A rename is a deletion and an addition here, as
                // `--name-only` without `-M` lists it: both paths moved.
                options.track_rewrites(None);
            })
            .for_each_to_obtain_tree(&tree, |change| {
                // `--name-only` lists files. A directory that appears or
                // vanishes arrives here as its own entry, and is not one.
                if change.entry_mode().is_tree() {
                    return Ok::<_, std::convert::Infallible>(std::ops::ControlFlow::Continue(()));
                }
                let location = change.location().to_str_lossy();
                if location.starts_with(prefix) {
                    paths.push(location.into_owned());
                }
                Ok(std::ops::ControlFlow::Continue(()))
            })
            .map_err(|err| walk_error(&err))?;
        out.push(TouchedCommit {
            revision: revision_of(&commit)?,
            paths,
        });
    }
    Ok(out)
}

/// The commit `rev` names, read from a full hex id; `None` when this copy
/// does not hold it as a commit.
fn commit_by_id<'repo>(repo: &'repo gix::Repository, rev: &str) -> Option<gix::Commit<'repo>> {
    let id = gix::ObjectId::from_hex(rev.as_bytes()).ok()?;
    repo.find_object(id).ok()?.try_into_commit().ok()
}

/// Whether this copy holds the commit `id` (a full hex object id).
pub fn holds_commit(repo_path: &Path, id: &str) -> Result<bool> {
    let repo = open(repo_path)?;
    let held = commit_by_id(&repo, id).is_some();
    Ok(held)
}

/// Walk every commit `HEAD` reaches — whatever its time says: a commit's
/// time is its committer's clock, and an ancestor may be dated after its
/// child — calling `seen` on each until it says stop. An object the walk
/// cannot read is an error, never the end of the history.
fn each_reached(
    repo: &gix::Repository,
    mut seen: impl FnMut(&gix::revision::walk::Info<'_>) -> Result<bool>,
) -> Result<()> {
    let Some(tip) = head(repo)? else {
        return Ok(());
    };
    let walk = repo.rev_walk([tip]).all().map_err(|err| walk_error(&err))?;
    for info in walk {
        if !seen(&info.map_err(|err| walk_error(&err))?)? {
            break;
        }
    }
    Ok(())
}

/// Whether the commit `id` (full hex) is `HEAD` or one of its ancestors,
/// by the commit graph alone. An error when it cannot be known: `id` is
/// not a commit this copy can read, or the history is not readable.
pub fn reaches(repo_path: &Path, id: &str) -> Result<bool> {
    let repo = open(repo_path)?;
    let target = gix::ObjectId::from_hex(id.as_bytes())
        .map_err(|err| SyncError::Git(format!("{id} is not a commit id: {err}")))?;
    repo.find_object(target)
        .map_err(|err| walk_error(&err))?
        .try_into_commit()
        .map_err(|err| walk_error(&err))?;
    let mut found = false;
    each_reached(&repo, |info| {
        found = info.id == target;
        Ok(!found)
    })?;
    Ok(found)
}

/// Which of `lines` the messages of `HEAD`'s whole history hold as one of
/// their own lines: one walk for all of them, stopped once each is found.
pub fn lines_in_history(repo_path: &Path, lines: &HashSet<String>) -> Result<HashSet<String>> {
    let repo = open(repo_path)?;
    let mut found = HashSet::new();
    if lines.is_empty() {
        return Ok(found);
    }
    each_reached(&repo, |info| {
        let commit = info.object().map_err(|err| walk_error(&err))?;
        let message = commit.message_raw().map_err(|err| walk_error(&err))?;
        for held in message.to_str_lossy().lines() {
            if lines.contains(held) {
                found.insert(held.to_owned());
            }
        }
        Ok(found.len() < lines.len())
    })?;
    Ok(found)
}

/// The files that differ between the trees of commits `from` and `to`,
/// repository-relative and `/`-separated, each once. `from` = `None` is the
/// empty tree: everything `to` holds. A rename is both paths, as on
/// [`recent_commits`].
pub fn changed_between(repo_path: &Path, from: Option<&str>, to: &str) -> Result<Vec<String>> {
    let repo = open(repo_path)?;
    let missing = |id: &str| SyncError::Git(format!("this copy does not hold the commit {id}"));
    let to_tree = commit_by_id(&repo, to)
        .ok_or_else(|| missing(to))?
        .tree()
        .map_err(|err| walk_error(&err))?;
    let from_tree = match from {
        Some(from) => commit_by_id(&repo, from)
            .ok_or_else(|| missing(from))?
            .tree()
            .map_err(|err| walk_error(&err))?,
        None => repo.empty_tree(),
    };
    let mut paths = Vec::new();
    from_tree
        .changes()
        .map_err(|err| walk_error(&err))?
        .options(|options| {
            options.track_path();
            options.track_rewrites(None);
        })
        .for_each_to_obtain_tree(&to_tree, |change| {
            if !change.entry_mode().is_tree() {
                paths.push(change.location().to_str_lossy().into_owned());
            }
            Ok::<_, std::convert::Infallible>(std::ops::ControlFlow::Continue(()))
        })
        .map_err(|err| walk_error(&err))?;
    paths.sort();
    paths.dedup();
    Ok(paths)
}

/// One path's bytes as of `rev`, or `None` where that revision does not hold
/// the path. `rev` is anything `git rev-parse` accepts — a full or abbreviated
pub fn blob_at(repo_path: &Path, rev: &str, rel: &str) -> Result<Option<Vec<u8>>> {
    let repo = open(repo_path)?;
    let Ok(id) = repo.rev_parse_single(rev) else {
        return Ok(None);
    };
    let Ok(object) = id.object() else {
        return Ok(None);
    };
    let Ok(commit) = object.peel_to_kind(gix::object::Kind::Commit) else {
        return Ok(None);
    };
    let tree = commit
        .into_commit()
        .tree()
        .map_err(|err| walk_error(&err))?;
    let Some(entry) = tree
        .lookup_entry_by_path(rel)
        .map_err(|err| walk_error(&err))?
    else {
        return Ok(None);
    };
    let blob = entry.object().map_err(|err| walk_error(&err))?;
    Ok(Some(blob.detach().data))
}

/// The unified diff of `rel` from `from_rev` to `to_rev`, or to the working
/// tree when `to_rev` is `None` — the hunks and nothing else, as
/// `git diff --unified=3 <from> [<to>] -- <rel>` prints after its header.
///
/// A side that does not hold the path is the empty text, so a note added
/// after `from_rev` diffs as all additions, and one removed since as all
/// removals. Bytes that are not UTF-8 are read lossily: a note is UTF-8 by
/// construction, and a diff of a file that is not is a diff nobody will read.
pub fn unified_diff(
    repo_path: &Path,
    rel: &str,
    from_rev: &str,
    to_rev: Option<&str>,
) -> Result<String> {
    use gix::diff::blob::{
        unified_diff::{ConsumeBinaryHunk, ContextSize},
        Algorithm, InternedInput, UnifiedDiff,
    };

    let before = blob_at(repo_path, from_rev, rel)?.unwrap_or_default();
    let after = match to_rev {
        Some(rev) => blob_at(repo_path, rev, rel)?.unwrap_or_default(),
        None => std::fs::read(repo_path.join(rel)).unwrap_or_default(),
    };
    let before = String::from_utf8_lossy(&before);
    let after = String::from_utf8_lossy(&after);
    let input = InternedInput::new(before.as_ref(), after.as_ref());
    let diff = gix::diff::blob::diff_with_slider_heuristics(Algorithm::Histogram, &input);
    UnifiedDiff::new(
        &diff,
        &input,
        ConsumeBinaryHunk::new(String::new(), "\n"),
        ContextSize::symmetrical(3),
    )
    .consume()
    .map_err(|err| SyncError::Git(format!("could not render the diff: {err}")))
}

/// Every path under `prefix` whose bytes on disk are not what `HEAD` holds:
/// added, modified, deleted, or never tracked at all. Repository-relative,
/// `/`-separated, prefix included — the paths `git status --porcelain` names.
pub fn dirty_paths(repo_path: &Path, prefix: &str) -> Result<HashSet<String>> {
    let repo = open(repo_path)?;
    let status = status_paths_for(&repo, "history")?;
    Ok(status
        .added
        .iter()
        .chain(&status.modified)
        .chain(&status.deleted)
        .chain(&status.untracked)
        .map(|path| {
            path.components()
                .map(|component| component.as_os_str().to_string_lossy())
                .collect::<Vec<_>>()
                .join("/")
        })
        .filter(|path| path.starts_with(prefix))
        .collect())
}

#[cfg(test)]
mod tests {
    use std::{path::PathBuf, process::Command};

    use super::*;

    /// A `git` that reads no configuration but the repository's own, with a
    /// fixed identity and a clock that ticks by the second so commit times
    /// order the way the walk sorts them. `None` where no `git` is on `PATH`.
    struct Repo {
        dir: PathBuf,
        tick: std::cell::Cell<i64>,
    }

    impl Repo {
        fn init(dir: &Path) -> Option<Self> {
            let repo = Self {
                dir: dir.to_path_buf(),
                tick: std::cell::Cell::new(1_700_000_000),
            };
            repo.try_git(&["init", "-q", "-b", "main"]).then_some(repo)
        }

        fn command(&self) -> Command {
            let stamp = format!("{} +0000", self.tick.get());
            let mut command = Command::new("git");
            command
                .current_dir(&self.dir)
                .env("GIT_CONFIG_NOSYSTEM", "1")
                .env("GIT_CONFIG_GLOBAL", "/dev/null")
                .env("GIT_AUTHOR_NAME", "t")
                .env("GIT_AUTHOR_EMAIL", "t@example.invalid")
                .env("GIT_COMMITTER_NAME", "t")
                .env("GIT_COMMITTER_EMAIL", "t@example.invalid")
                .env("GIT_AUTHOR_DATE", &stamp)
                .env("GIT_COMMITTER_DATE", &stamp);
            command
        }

        fn try_git(&self, args: &[&str]) -> bool {
            self.command()
                .args(args)
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status()
                .is_ok_and(|status| status.success())
        }

        fn git(&self, args: &[&str]) -> String {
            let output = self.command().args(args).output().expect("git");
            assert!(
                output.status.success(),
                "git {args:?}: {}",
                String::from_utf8_lossy(&output.stderr)
            );
            String::from_utf8_lossy(&output.stdout).trim().to_owned()
        }

        fn write(&self, rel: &str, text: &str) {
            let path = self.dir.join(rel);
            std::fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
            std::fs::write(path, text).expect("write");
        }

        /// Commit everything with `message`, one second later than the last.
        fn commit(&self, message: &str) -> String {
            self.tick.set(self.tick.get() + 1);
            self.git(&["add", "-A"]);
            self.git(&["commit", "-q", "--allow-empty", "-m", message]);
            self.git(&["rev-parse", "HEAD"])
        }
    }

    /// Three commits: the note is added, an unrelated file is added, the note
    /// is rewritten. The log names the first and the third, newest first,
    /// with the ids, times and whole messages `git log` would print.
    #[test]
    fn a_file_log_names_the_commits_that_changed_the_path_newest_first() {
        let dir = tempfile::tempdir().expect("tempdir");
        let Some(repo) = Repo::init(dir.path()) else {
            return;
        };
        repo.write("notes/a.md", "one\n");
        let added = repo.commit("add a\n\nKeeper-Device: mac\n");
        repo.write("notes/b.md", "other\n");
        repo.commit("add b");
        repo.write("notes/a.md", "one, revised\n");
        let revised = repo.commit("revise a");

        let log = file_log(dir.path(), "notes/a.md", 10).expect("log");
        assert_eq!(
            log.iter().map(|r| r.id.as_str()).collect::<Vec<_>>(),
            vec![revised.as_str(), added.as_str()]
        );
        assert_eq!(log[0].subject(), "revise a");
        assert_eq!(log[1].message, "add a\n\nKeeper-Device: mac\n");
        assert!(log[0].committed_secs > log[1].committed_secs);
        // Bounded, newest kept.
        assert_eq!(
            file_log(dir.path(), "notes/a.md", 1).expect("log"),
            vec![log[0].clone()]
        );
        // A path nobody committed, and a repository with no commit at all.
        assert!(file_log(dir.path(), "notes/none.md", 10)
            .expect("log")
            .is_empty());
        let empty = tempfile::tempdir().expect("tempdir");
        let Some(_) = Repo::init(empty.path()) else {
            return;
        };
        assert!(file_log(empty.path(), "notes/a.md", 10)
            .expect("an unborn branch is an empty history")
            .is_empty());
    }

    /// What one commit holds under a folder, from git's objects: every file
    /// at any depth with its id, size and kind, sorted — a link is listed
    /// as not plain, and what came later or is only on the disk is not.
    #[test]
    fn files_at_list_one_commits_folder() {
        let dir = tempfile::tempdir().expect("tempdir");
        let Some(repo) = Repo::init(dir.path()) else {
            return;
        };
        repo.write("flows/b.md", "bee\n");
        repo.write("flows/deep/a.md", "a\n");
        repo.write("other.md", "x\n");
        std::os::unix::fs::symlink("/etc", dir.path().join("flows/link")).expect("link");
        let first = repo.commit("first");
        repo.write("flows/c.md", "later\n");
        repo.commit("second");
        repo.write("flows/d.md", "uncommitted\n");

        let files = files_at(dir.path(), &first, "flows").expect("list");
        let paths: Vec<(&str, u64, bool)> = files
            .iter()
            .map(|file| (file.path.as_str(), file.size, file.regular))
            .collect();
        assert_eq!(
            paths,
            [
                ("flows/b.md", 4, true),
                ("flows/deep/a.md", 2, true),
                ("flows/link", 4, false)
            ]
        );
        assert_eq!(files[0].id, crate::engine::blob_id(b"bee\n"));
        assert!(files_at(dir.path(), &first, "none")
            .expect("list")
            .is_empty());
        assert!(files_at(
            dir.path(),
            "0000000000000000000000000000000000000000",
            "flows"
        )
        .is_err());
    }

    /// R95U-09: a merge that took a path as one side had it is not a change
    /// of its own — the side's commit is — while a merge that made the path
    /// differ from both sides is; the walk starts at the revision asked.
    #[test]
    fn a_merge_carrying_one_sides_change_is_not_a_change_of_its_own() {
        let dir = tempfile::tempdir().expect("tempdir");
        let Some(repo) = Repo::init(dir.path()) else {
            return;
        };
        repo.write("skill/SKILL.md", "one\n");
        let created = repo.commit("create");
        repo.git(&["checkout", "-q", "-b", "side"]);
        repo.write("skill/SKILL.md", "marked\n");
        let marked = repo.commit("mark");
        repo.git(&["checkout", "-q", "main"]);
        repo.write("other.md", "o\n");
        let other = repo.commit("other");
        repo.tick.set(repo.tick.get() + 1);
        repo.git(&["merge", "-q", "--no-ff", "-m", "merge side", "side"]);
        let merged = repo.git(&["rev-parse", "HEAD"]);

        let ids = |rev: &str| -> Vec<String> {
            path_changes(dir.path(), rev, "skill", 10)
                .expect("log")
                .into_iter()
                .map(|revision| revision.id)
                .collect()
        };
        assert_eq!(ids(&merged), [marked.clone(), created.clone()]);
        assert_eq!(
            ids(&other),
            std::slice::from_ref(&created),
            "pinned to the revision asked"
        );
        assert_eq!(
            path_changes(dir.path(), &merged, "skill", 1)
                .expect("log")
                .len(),
            1
        );

        repo.git(&["checkout", "-q", "-b", "again", &created]);
        repo.write("skill/SKILL.md", "theirs\n");
        repo.commit("theirs");
        repo.git(&["checkout", "-q", "main"]);
        repo.write("skill/SKILL.md", "ours\n");
        repo.commit("ours");
        repo.tick.set(repo.tick.get() + 1);
        let _ = repo.try_git(&["merge", "-q", "-m", "resolve", "again"]);
        repo.write("skill/SKILL.md", "resolved\n");
        let resolved = repo.commit("resolve");
        assert_eq!(
            ids(&resolved)[0],
            resolved,
            "a resolution is a change of its own"
        );
    }

    /// R95U2-02: a merge that kept one parent's version follows that parent
    /// only — the side it discarded is neither listed nor walked, however
    /// new its commits are; a merge that took the side's version lists the
    /// side's commit.
    #[test]
    fn a_discarded_side_is_no_change() {
        let dir = tempfile::tempdir().expect("tempdir");
        let Some(repo) = Repo::init(dir.path()) else {
            return;
        };
        repo.write("skill/SKILL.md", "one\n");
        let created = repo.commit("create");
        repo.git(&["checkout", "-q", "-b", "side"]);
        repo.write("skill/SKILL.md", "rejected\n");
        let rejected = repo.commit("rejected patch");
        repo.git(&["checkout", "-q", "main"]);
        repo.write("other.md", "o\n");
        repo.commit("other");
        repo.tick.set(repo.tick.get() + 1);
        repo.git(&[
            "merge",
            "-q",
            "--no-ff",
            "-s",
            "ours",
            "-m",
            "keep ours",
            "side",
        ]);
        let kept = repo.git(&["rev-parse", "HEAD"]);
        let ids = |rev: &str| -> Vec<String> {
            path_changes(dir.path(), rev, "skill", 10)
                .expect("log")
                .into_iter()
                .map(|revision| revision.id)
                .collect()
        };
        assert_eq!(ids(&kept), std::slice::from_ref(&created));
        assert_eq!(
            ids(&rejected),
            [rejected.clone(), created.clone()],
            "the side's own history still has it"
        );
    }

    /// A deletion is a change to the path too, and it comes back as such.
    #[test]
    fn a_removal_is_a_revision_of_the_path() {
        let dir = tempfile::tempdir().expect("tempdir");
        let Some(repo) = Repo::init(dir.path()) else {
            return;
        };
        repo.write("notes/a.md", "one\n");
        repo.commit("add a");
        std::fs::remove_file(dir.path().join("notes/a.md")).expect("rm");
        let removed = repo.commit("remove a");
        let log = file_log(dir.path(), "notes/a.md", 10).expect("log");
        assert_eq!(log.len(), 2);
        assert_eq!(log[0].id, removed);
        assert_eq!(
            blob_at(dir.path(), &removed, "notes/a.md").expect("read"),
            None,
            "the revision that removed it does not hold it"
        );
    }

    /// The unread projection's window: every commit, newest first, with the
    /// paths under the vault subfolder each touched — and only those.
    #[test]
    fn recent_commits_name_the_paths_under_the_prefix_each_touched() {
        let dir = tempfile::tempdir().expect("tempdir");
        let Some(repo) = Repo::init(dir.path()) else {
            return;
        };
        repo.write("notes/a.md", "one\n");
        repo.write("README.md", "outside the vault\n");
        let first = repo.commit("first");
        repo.write("notes/a.md", "one, revised\n");
        repo.write("notes/deep/c.md", "deep\n");
        let second = repo.commit("second");
        repo.write("README.md", "still outside\n");
        let third = repo.commit("third");

        let recent = recent_commits(dir.path(), "notes/", 10).expect("recent");
        let ids: Vec<&str> = recent.iter().map(|c| c.revision.id.as_str()).collect();
        assert_eq!(ids, vec![third.as_str(), second.as_str(), first.as_str()]);
        assert!(recent[0].paths.is_empty(), "the README is not under notes/");
        let mut second_paths = recent[1].paths.clone();
        second_paths.sort();
        assert_eq!(second_paths, vec!["notes/a.md", "notes/deep/c.md"]);
        assert_eq!(recent[2].paths, vec!["notes/a.md"]);
        assert_eq!(
            recent_commits(dir.path(), "notes/", 2)
                .expect("recent")
                .len(),
            2,
            "the limit counts commits"
        );
    }

    /// `blob_at` reads what `git show <rev>:<path>` prints, for a full id,
    /// an abbreviated one and `HEAD`; a revision that names nothing is `None`.
    #[test]
    fn a_blob_at_a_revision_is_the_bytes_git_show_prints() {
        let dir = tempfile::tempdir().expect("tempdir");
        let Some(repo) = Repo::init(dir.path()) else {
            return;
        };
        repo.write("notes/a.md", "one\n");
        let first = repo.commit("first");
        repo.write("notes/a.md", "two\n");
        repo.commit("second");
        assert_eq!(
            blob_at(dir.path(), &first, "notes/a.md").expect("read"),
            Some(b"one\n".to_vec())
        );
        assert_eq!(
            blob_at(dir.path(), &first[..10], "notes/a.md").expect("read"),
            Some(b"one\n".to_vec())
        );
        assert_eq!(
            blob_at(dir.path(), "HEAD", "notes/a.md").expect("read"),
            Some(b"two\n".to_vec())
        );
        assert_eq!(
            blob_at(
                dir.path(),
                "0000000000000000000000000000000000000000",
                "notes/a.md"
            )
            .expect("a revision that is not there reads as nothing"),
            None
        );
    }

    /// The hunks are what `git diff --unified=3` prints — the same header
    /// arithmetic, the same prefixes — between two revisions and against the
    /// working tree.
    #[test]
    fn a_unified_diff_matches_git_diff() {
        let dir = tempfile::tempdir().expect("tempdir");
        let Some(repo) = Repo::init(dir.path()) else {
            return;
        };
        let body: String = (1..=12).map(|n| format!("line {n}\n")).collect();
        repo.write("notes/a.md", &body);
        let first = repo.commit("first");
        let edited = body.replace("line 6\n", "line six\n");
        repo.write("notes/a.md", &edited);
        let second = repo.commit("second");
        // Uncommitted on top, for the working-tree half.
        repo.write("notes/a.md", &format!("{edited}line 13\n"));

        let ours = unified_diff(dir.path(), "notes/a.md", &first, Some(&second)).expect("diff");
        let theirs = repo.git(&[
            "diff",
            "--no-color",
            "--unified=3",
            &first,
            &second,
            "--",
            "notes/a.md",
        ]);
        // git appends the enclosing "function" line after the second `@@`;
        // the hunk parser on the far side reads the four numbers and ignores
        // it, so the comparison does too.
        let theirs_hunks: String = theirs
            .lines()
            .skip_while(|line| !line.starts_with("@@"))
            .map(|line| match line.strip_prefix("@@ ") {
                Some(header) => format!("@@ {} @@\n", header.split(" @@").next().unwrap_or("")),
                None => format!("{line}\n"),
            })
            .collect();
        assert_eq!(ours, theirs_hunks);
        assert!(ours.starts_with("@@ -3,7 +3,7 @@"));

        let working = unified_diff(dir.path(), "notes/a.md", &second, None).expect("diff");
        assert!(working.contains("+line 13"), "{working}");
        let added = unified_diff(dir.path(), "notes/new.md", &first, None).expect("diff");
        assert!(added.is_empty(), "a path on neither side has no hunks");
        std::fs::write(dir.path().join("notes/new.md"), "fresh\n").expect("write");
        let added = unified_diff(dir.path(), "notes/new.md", &first, None).expect("diff");
        // gitoxide spells an empty side `-1,0` where git spells it `-0,0`;
        // the four numbers parse the same on the far side.
        assert_eq!(added, "@@ -1,0 +1,1 @@\n+fresh\n");
    }

    /// The dirty set names what `git status` names under the prefix and
    /// nothing outside it.
    #[test]
    fn dirty_paths_are_the_status_under_the_prefix() {
        let dir = tempfile::tempdir().expect("tempdir");
        let Some(repo) = Repo::init(dir.path()) else {
            return;
        };
        repo.write("notes/kept.md", "kept\n");
        repo.write("notes/edited.md", "before\n");
        repo.write("notes/gone.md", "gone\n");
        repo.write("README.md", "outside\n");
        repo.commit("first");
        repo.write("notes/edited.md", "after\n");
        repo.write("notes/new.md", "new\n");
        repo.write("README.md", "outside, edited\n");
        std::fs::remove_file(dir.path().join("notes/gone.md")).expect("rm");

        let mut dirty: Vec<String> = dirty_paths(dir.path(), "notes/")
            .expect("status")
            .into_iter()
            .collect();
        dirty.sort();
        assert_eq!(
            dirty,
            vec!["notes/edited.md", "notes/gone.md", "notes/new.md"]
        );
    }
}
